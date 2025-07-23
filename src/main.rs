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
        .add_systems(
            Update,
            (
                handle_chunk_gen,
                handle_chunk_despawn
                    .run_if(|game_settings: Res<GameSettings>| game_settings.despawn_chunks),
                process_tasks,
                apply_chunk_mesh,
            ),
        )
        .run();
}

#[derive(Resource, Clone)]
pub struct GameInfo {
    pub seed: u32,
    pub chunks: Arc<RwLock<HashMap<IVec3, Chunk>>>,
    pub loading_chunks: Arc<RwLock<HashSet<IVec3>>>,
    pub materials: Vec<Handle<StandardMaterial>>,
}

#[derive(Reflect, Resource, Default)]
pub struct GameSettings {
    pub movement_speed: f32,
    pub render_distance: i32,
    pub despawn_chunks: bool,
}

pub const CHUNK_SIZE: i32 = 16; // MAX 63
pub const CHUNK_HEIGHT: i32 = 256; // MAX 511
pub const SEA_LEVEL: i32 = 64;

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
        render_distance: 16,
        despawn_chunks: true,
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

pub fn handle_chunk_gen(
    mut commands: Commands,
    mut movement_settings: ResMut<MovementSettings>,
    game_info: Res<GameInfo>,
    game_settings: Res<GameSettings>,
    player: Single<&Transform, With<Camera3d>>,
) {
    movement_settings.speed = game_settings.movement_speed;
    let pt = player.translation;
    let thread_pool = AsyncComputeTaskPool::get();
    let render_distance = game_settings.render_distance;

    for chunk_z in
        (pt.z as i32 / CHUNK_SIZE - render_distance)..(pt.z as i32 / CHUNK_SIZE + render_distance)
    {
        for chunk_x in (pt.x as i32 / CHUNK_SIZE - render_distance)
            ..(pt.x as i32 / CHUNK_SIZE + render_distance)
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
                                    BlockKind::Grass
                                } else if y < max_y {
                                    match y {
                                        _ if y > 150 => BlockKind::Stone,
                                        _ if y == max_y - 1 => BlockKind::Grass,
                                        _ if y >= max_y - 4 => BlockKind::Dirt,
                                        _ => BlockKind::Stone,
                                    }
                                } else if y < SEA_LEVEL {
                                    BlockKind::Plank
                                } else {
                                    BlockKind::Air
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
                                        let local_max_y = terrain_noise(
                                            (chunk.pos * CHUNK_SIZE + pos).as_vec3().xz(),
                                            seed,
                                        );

                                        pos.y += local_max_y as i32;

                                        if (0..CHUNK_SIZE).contains(&pos.x)
                                            && (0..CHUNK_HEIGHT).contains(&pos.y)
                                            && (0..CHUNK_SIZE).contains(&pos.z)
                                        {
                                            chunk.blocks[vec3_to_index(pos)] = *block;
                                        } else if let Some(relative_chunk_key) =
                                            chunk.get_relative_chunk(pos)
                                            && let Some(target_chunk) = chunks_for_task
                                                .write()
                                                .unwrap()
                                                .get_mut(&relative_chunk_key)
                                        {
                                            let block_index = vec3_to_index(
                                                pos - relative_chunk_key * CHUNK_SIZE,
                                            );
                                            if block_index < target_chunk.blocks.len() {
                                                target_chunk.blocks[block_index] = *block;
                                            }
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
}

fn process_tasks(
    mut commands: Commands,
    mut spawn_tasks: Query<(Entity, &mut ComputeChunk)>,
    mut despawn_tasks: Query<(Entity, &mut ComputeDespawn)>,
    game_info: Res<GameInfo>,
) {
    let mut processed_this_frame = 0;
    for (entity, mut compute_task) in spawn_tasks.iter_mut() {
        if processed_this_frame >= 250 {
            break;
        }
        if let Some(result) = future::block_on(future::poll_once(&mut compute_task.0)) {
            commands
                .entity(entity)
                .try_insert((
                    ChunkEntity,
                    Transform::from_translation((result.1 * CHUNK_SIZE).as_vec3()),
                ))
                .try_remove::<ComputeChunk>();

            game_info.chunks.write().unwrap().insert(result.1, result.0);
            game_info.loading_chunks.write().unwrap().remove(&result.1);

            processed_this_frame += 1;
        }
    }

    let mut processed_this_frame = 0;
    for (task_entity, mut compute_task) in despawn_tasks.iter_mut() {
        if processed_this_frame >= 1 {
            break;
        }
        if let Some(despawn_list) = future::block_on(future::poll_once(&mut compute_task.0)) {
            let mut game_chunks_write_guard = game_info.chunks.write().unwrap();
            let mut game_loading_chunks_write_guard = game_info.loading_chunks.write().unwrap();
            for (entity_to_despawn, chunk_key) in despawn_list {
                commands.entity(entity_to_despawn).try_despawn();
                game_chunks_write_guard.remove(&chunk_key);
                game_loading_chunks_write_guard.remove(&chunk_key);
            }
            commands.entity(task_entity).try_despawn();
            processed_this_frame += 1;
        }
    }
}

fn handle_chunk_despawn(
    mut commands: Commands,
    game_settings: Res<GameSettings>,
    chunks_query: Query<(Entity, &Transform), With<ChunkEntity>>,
    player: Single<&Transform, With<Camera3d>>,
) {
    let pt = player.translation;
    let thread_pool = AsyncComputeTaskPool::get();
    let render_distance = game_settings.render_distance;

    let mut chunks_to_check: Vec<(IVec3, Entity)> = Vec::new();
    for (entity, transform) in chunks_query.iter() {
        let chunk_key = transform.translation.as_ivec3() / CHUNK_SIZE;
        chunks_to_check.push((chunk_key, entity));
    }

    if chunks_to_check.is_empty() {
        return;
    }

    let task = thread_pool.spawn(async move {
        let mut despawn_list: Vec<(Entity, IVec3)> = Vec::new();
        for (chunk_key, entity) in chunks_to_check {
            if (chunk_key.x + render_distance < pt.x as i32 / CHUNK_SIZE)
                || (chunk_key.x - render_distance > pt.x as i32 / CHUNK_SIZE)
                || (chunk_key.z + render_distance < pt.z as i32 / CHUNK_SIZE)
                || (chunk_key.z - render_distance > pt.z as i32 / CHUNK_SIZE)
            {
                despawn_list.push((entity, chunk_key));
            }
        }
        despawn_list
    });
    commands.spawn(ComputeDespawn(task));
}

#[derive(Component)]
struct ComputeChunk(Task<(Chunk, IVec3)>);

#[derive(Component)]
struct ComputeDespawn(Task<Vec<(Entity, IVec3)>>);

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

        commands.entity(entity).try_insert(ComputeChunkMesh(task));
    }

    let mut processed_this_frame = 0;

    for (entity, mut compute_task) in task_query.iter_mut() {
        if processed_this_frame >= 5 {
            break;
        }

        if let Some(result) = future::block_on(future::poll_once(&mut compute_task.0)) {
            commands.entity(entity).try_remove::<ComputeChunkMesh>();

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

                commands.entity(entity).try_insert((
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
