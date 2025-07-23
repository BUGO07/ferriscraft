use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use bevy::{
    asset::RenderAssetUsages,
    diagnostic::{EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin},
    prelude::*,
    render::mesh::{Indices, PrimitiveTopology},
    tasks::{AsyncComputeTaskPool, Task, futures_lite::future},
    window::WindowMode,
};
use bevy_flycam::{FlyCam, MovementSettings, NoCameraPlayerPlugin};
use bevy_inspector_egui::{
    bevy_egui::EguiPlugin,
    quick::{ResourceInspectorPlugin, WorldInspectorPlugin},
};
use iyes_perf_ui::{PerfUiPlugin, prelude::PerfUiAllEntries};

use crate::{
    mesher::{Chunk, ChunkEntity, ChunkMesh, build_chunk_mesh},
    utils::{
        Block, BlockKind, TREE_OBJECT, get_vertex_u32, terrain_noise, tree_noise, vec3_to_index,
    },
};

pub mod mesher;
pub mod utils;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "FerrisCraft".to_string(),
                        mode: WindowMode::Windowed,
                        present_mode: bevy::window::PresentMode::AutoNoVsync,
                        ..default()
                    }),
                    ..default()
                })
                .set(ImagePlugin::default_nearest()), // for low res textures
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin,
            PerfUiPlugin,
            EguiPlugin {
                enable_multipass_for_primary_context: false,
            },
            WorldInspectorPlugin::default(),
            ResourceInspectorPlugin::<GameSettings>::default(),
            NoCameraPlayerPlugin,
        ))
        .init_resource::<MovementSettings>()
        .add_systems(Startup, setup)
        .add_systems(Update, (generate_chunks, apply_chunk_mesh))
        .run();
}

#[derive(Resource, Clone)]
pub struct GameInfo {
    pub seed: u32,
    pub chunks: Arc<RwLock<HashMap<IVec3, Chunk>>>,
    pub loading_chunks: Arc<RwLock<HashSet<IVec3>>>,
    pub materials: Vec<Handle<StandardMaterial>>,
}

#[derive(Resource, Reflect, Default)]
pub struct GameSettings {
    pub movement_speed: f32,
}

pub const CHUNK_SIZE: i32 = 16; // MAX 63
pub const CHUNK_HEIGHT: i32 = 256; // MAX 511
pub const SEA_LEVEL: i32 = 64;
pub const RENDER_DISTANCE: i32 = 16;

fn setup(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    let seed = 0;
    commands.insert_resource(GameInfo {
        seed,
        chunks: Arc::new(RwLock::new(HashMap::new())),
        loading_chunks: Arc::new(RwLock::new(HashSet::new())),
        materials: vec![materials.add(StandardMaterial {
            base_color_texture: Some(asset_server.load("atlas.png")),
            ..default()
        })],
    });
    commands.insert_resource(GameSettings {
        movement_speed: 200.0,
    });

    commands.spawn((
        Camera3d::default(),
        FlyCam,
        Transform::from_xyz(5.0, CHUNK_HEIGHT as f32, -5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn(PerfUiAllEntries::default());
    commands.spawn((
        DirectionalLight {
            illuminance: 5_000.0,
            shadows_enabled: true,
            ..default()
        },
        // light idk
        Transform::from_rotation(Quat::from_euler(
            EulerRot::ZYX,
            0.0,
            std::f32::consts::FRAC_PI_4,  // 45 degrees around Y
            -std::f32::consts::FRAC_PI_3, // -60 degrees pitch (sun in sky)
        )),
    ));
}

fn generate_chunks(
    mut commands: Commands,
    mut movement_settings: ResMut<MovementSettings>,
    mut task_query: Query<(Entity, &mut ComputeChunk)>,
    game_info: Res<GameInfo>,
    game_settings: Res<GameSettings>,
    // chunks: Query<(Entity, &Transform), With<ChunkEntity>>,
    player: Single<&Transform, With<Camera3d>>,
) {
    movement_settings.speed = game_settings.movement_speed;
    let pt = player.translation;
    let thread_pool = AsyncComputeTaskPool::get();

    // for chunk in game_info.chunks.read().unwrap().keys() {
    //     // check if its far from the player and if it is despawn it
    //     if (chunk.x + RENDER_DISTANCE < pt.x as i32 / CHUNK_SIZE)
    //         || (chunk.x - RENDER_DISTANCE > pt.x as i32 / CHUNK_SIZE)
    //         || (chunk.z + RENDER_DISTANCE < pt.z as i32 / CHUNK_SIZE)
    //         || (chunk.z - RENDER_DISTANCE > pt.z as i32 / CHUNK_SIZE)
    //     {
    //         commands
    //             .entity(
    //                 chunks
    //                     .iter()
    //                     .find(|x| x.1.translation.as_ivec3() / CHUNK_SIZE == *chunk)
    //                     .unwrap()
    //                     .0,
    //             )
    //             .despawn();
    //         game_info.chunks.write().unwrap().remove(chunk);
    //     }
    // }
    for chunk_z in
        (pt.z as i32 / CHUNK_SIZE - RENDER_DISTANCE)..(pt.z as i32 / CHUNK_SIZE + RENDER_DISTANCE)
    {
        for chunk_x in (pt.x as i32 / CHUNK_SIZE - RENDER_DISTANCE)
            ..(pt.x as i32 / CHUNK_SIZE + RENDER_DISTANCE)
        {
            let current_chunk_key = ivec3(chunk_x, 0, chunk_z);

            let chunks_guard = game_info.chunks.read().unwrap();
            let loading_chunks_guard = game_info.loading_chunks.read().unwrap();

            if chunks_guard.contains_key(&current_chunk_key)
                || loading_chunks_guard.contains(&current_chunk_key)
            {
                continue;
            }
            drop(chunks_guard);
            drop(loading_chunks_guard);

            game_info
                .loading_chunks
                .write()
                .unwrap()
                .insert(current_chunk_key);

            let seed = game_info.seed;
            let chunks_for_task = game_info.chunks.clone();
            let task = thread_pool.spawn(async move {
                let mut chunk = Chunk::new(ivec3(chunk_x, 0, chunk_z));

                for rela_z in 0..CHUNK_SIZE {
                    for rela_x in 0..CHUNK_SIZE {
                        let pos = vec2(
                            (rela_x + chunk_x * CHUNK_SIZE) as f32,
                            (rela_z + chunk_z * CHUNK_SIZE) as f32,
                        );
                        let max_y = terrain_noise(pos, seed);

                        for y in 0..CHUNK_HEIGHT {
                            chunk.blocks[vec3_to_index(ivec3(rela_x, y, rela_z))] = Block {
                                kind: if y == 0 {
                                    // Always bedrock at the very bottom of the world
                                    BlockKind::Grass
                                } else if y < max_y {
                                    // If 'y' is below the terrain's surface (max_y),
                                    // assign Grass, Dirt, or Stone based on depth.
                                    match y {
                                        // Grass layer: The very top block of the solid terrain
                                        _ if y > 150 => BlockKind::Stone,
                                        _ if y == max_y - 1 => BlockKind::Grass,
                                        // Dirt layer: Typically 3 blocks below the surface, up to 1 block below grass
                                        _ if y >= max_y - 4 => BlockKind::Dirt, // Covers y from max_y - 4 to max_y - 2
                                        // Stone layer: Below the dirt layer, extending downwards
                                        _ => BlockKind::Stone, // Covers y < max_y - 4
                                    }
                                } else {
                                    // If 'y' is at or above the terrain's surface (max_y),
                                    // determine if it's air or bedrock (filling "water" areas).
                                    if y < SEA_LEVEL {
                                        // If below sea level, fill with Bedrock (mimicking water being replaced)
                                        BlockKind::Plank
                                    } else {
                                        // If at or above sea level, it's Air
                                        BlockKind::Air
                                    }
                                },
                            };
                        }

                        let tree_probabilty = tree_noise(pos, seed);

                        if tree_probabilty > 0.85 && max_y < 90 && max_y > SEA_LEVEL + 2 {
                            for (y, tree_layer) in TREE_OBJECT.iter().enumerate() {
                                for (z, tree_row) in tree_layer.iter().enumerate() {
                                    for (x, block) in tree_row.iter().enumerate() {
                                        let mut pos =
                                            ivec3(3 + x as i32, 1 + y as i32, 3 + z as i32);
                                        // * shitty way of getting the ground height at the center of the tree
                                        let local_max_y = terrain_noise(
                                            // &game_info.perlin,
                                            (chunk.pos * CHUNK_SIZE + pos).as_vec3().xz(),
                                            seed,
                                        );

                                        pos.y += local_max_y;

                                        if (0..CHUNK_SIZE).contains(&pos.x)
                                            && (0..CHUNK_HEIGHT).contains(&pos.y)
                                            && (0..CHUNK_SIZE).contains(&pos.z)
                                        {
                                            chunk.blocks[vec3_to_index(pos)] = *block;
                                        } else {
                                            // TODO this isn't a proper way to do it
                                            chunks_for_task
                                                .write()
                                                .unwrap()
                                                .get_mut(&chunk.get_relative_chunk(pos).unwrap())
                                                .unwrap()
                                                .blocks
                                                .insert(vec3_to_index(pos), *block);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                (chunk, ivec3(chunk_x, 0, chunk_z))
            });
            commands.spawn(ComputeChunk(task));
        }
    }

    let mut processed_this_frame = 0;
    for (entity, mut compute_task) in task_query.iter_mut() {
        if processed_this_frame >= 50 {
            break;
        }
        if let Some(result) = future::block_on(future::poll_once(&mut compute_task.0)) {
            commands
                .entity(entity)
                .insert((
                    ChunkEntity,
                    Transform::from_translation((result.1 * CHUNK_SIZE).as_vec3()),
                ))
                .remove::<ComputeChunk>();

            // Insert the completed chunk
            game_info.chunks.write().unwrap().insert(result.1, result.0);
            // Remove from loading_chunks as it's now fully processed and inserted
            game_info.loading_chunks.write().unwrap().remove(&result.1);

            processed_this_frame += 1;
        }
    }
}

#[derive(Component)]
struct ComputeChunk(Task<(Chunk, IVec3)>);

#[derive(Component)]
struct ComputeChunkMesh(Task<Option<ChunkMesh>>);

fn apply_chunk_mesh(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut task_query: Query<(Entity, &mut ComputeChunkMesh)>,
    game_info: Res<GameInfo>,
    chunks_query: Query<(Entity, &Transform), Added<ChunkEntity>>,
) {
    let thread_pool = AsyncComputeTaskPool::get();

    for (entity, chunk_transform) in chunks_query.iter() {
        let chunk_coords = chunk_transform.translation.as_ivec3() / CHUNK_SIZE;

        let chunks_for_task = game_info.chunks.clone();

        let task = thread_pool.spawn(async move {
            let chunks_map_guard = chunks_for_task.read().unwrap();

            let chunk_data_option = chunks_map_guard.get(&chunk_coords);

            if let Some(chunk_data) = chunk_data_option {
                build_chunk_mesh(chunk_data, &chunks_map_guard)
            } else {
                None
            }
        });

        commands.entity(entity).insert(ComputeChunkMesh(task));
    }

    let mut processed_this_frame = 0;

    for (entity, mut compute_task) in task_query.iter_mut() {
        if processed_this_frame >= 5 {
            break;
        }

        if let Some(result) = future::block_on(future::poll_once(&mut compute_task.0)) {
            commands.entity(entity).remove::<ComputeChunkMesh>();

            if let Some(mesh_data) = result {
                let mut bevy_mesh = Mesh::new(
                    PrimitiveTopology::TriangleList,
                    RenderAssetUsages::RENDER_WORLD,
                );
                let mut positions = Vec::new();
                let mut normals = Vec::new();

                for &vertex in mesh_data.vertices.iter() {
                    let (pos, _ao, normal, _block_type) = get_vertex_u32(vertex);
                    positions.push(pos);
                    normals.push(normal);
                }

                bevy_mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
                bevy_mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
                bevy_mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, mesh_data.uvs);
                bevy_mesh.insert_indices(Indices::U32(mesh_data.indices));

                let mesh_handle = meshes.add(bevy_mesh);

                commands.entity(entity).insert((
                    Mesh3d(mesh_handle),
                    MeshMaterial3d(game_info.materials[0].clone()),
                    Visibility::Visible,
                ));
            } else {
                error!("Error building chunk mesh for entity {:?}", entity);
            }
            processed_this_frame += 1;
        }
    }
}
