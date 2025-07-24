#![allow(clippy::too_many_arguments)]
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock},
};

use bevy::{
    asset::RenderAssetUsages,
    diagnostic::{
        EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
        SystemInformationDiagnosticsPlugin,
    },
    prelude::*,
    render::{
        diagnostic::RenderDiagnosticsPlugin,
        mesh::{Indices, PrimitiveTopology},
    },
    tasks::{AsyncComputeTaskPool, Task, futures_lite::future},
    window::{PrimaryWindow, WindowMode},
};
use bevy_flycam::{FlyCam, MovementSettings, NoCameraPlayerPlugin};
use bevy_inspector_egui::{
    bevy_egui::EguiPlugin,
    quick::{ResourceInspectorPlugin, WorldInspectorPlugin},
};
use iyes_perf_ui::{
    PerfUiPlugin,
    prelude::{PerfUiAllEntries, PerfUiEntryFPS},
};

use crate::{
    mesher::{
        Chunk, ChunkEntity, ChunkMesh, GameEntity, GameEntityKind, SavedChunk, build_chunk_mesh,
    },
    utils::{
        Block, BlockKind, TREE_OBJECT, ferris_noise, get_vertex_u32, place_block, ray_cast,
        terrain_noise, tree_noise, vec3_to_index,
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
            RenderDiagnosticsPlugin,
            SystemInformationDiagnosticsPlugin,
            PerfUiPlugin,
            EguiPlugin {
                enable_multipass_for_primary_context: false,
            },
            WorldInspectorPlugin::default().run_if(
                |perf_ui: Single<&Visibility, With<PerfUiEntryFPS>>| *perf_ui != Visibility::Hidden,
            ),
            ResourceInspectorPlugin::<GameSettings>::default().run_if(
                |perf_ui: Single<&Visibility, With<PerfUiEntryFPS>>| *perf_ui != Visibility::Hidden,
            ),
            NoCameraPlayerPlugin,
        ))
        .init_resource::<MovementSettings>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                update,
                handle_chunk_gen,
                handle_chunk_despawn
                    .run_if(|game_settings: Res<GameSettings>| game_settings.despawn_chunks),
                process_tasks,
                handle_mesh_gen,
            ),
        )
        .run();
}

#[derive(Resource)]
pub struct GameInfo {
    pub seed: u32,
    pub chunks: Arc<RwLock<HashMap<IVec3, Chunk>>>,
    pub loading_chunks: Arc<RwLock<HashSet<IVec3>>>,
    pub saved_chunks: Arc<RwLock<HashMap<IVec3, SavedChunk>>>,
    pub materials: Vec<Handle<StandardMaterial>>,
    pub models: Vec<Handle<Scene>>,
    pub despawn_tasks: Vec<Task<Vec<(Entity, IVec3)>>>,
    pub current_block: Block,
}

#[derive(Reflect, Resource, Default)]
pub struct GameSettings {
    pub movement_speed: f32,
    pub render_distance: i32,
    pub chunk_spawn_pf: i32,
    pub mesh_update_pf: i32,
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
    commands.insert_resource(GameInfo {
        seed: 0,
        chunks: Arc::new(RwLock::new(HashMap::new())),
        loading_chunks: Arc::new(RwLock::new(HashSet::new())),
        saved_chunks: Arc::new(RwLock::new(HashMap::new())),
        materials: vec![materials.add(StandardMaterial {
            base_color_texture: Some(asset_server.load("atlas.png")),
            ..default()
        })],
        models: vec![asset_server.load(GltfAssetLabel::Scene(0).from_asset("ferris.glb"))],
        despawn_tasks: Vec::new(),
        current_block: Block::STONE,
    });
    commands.insert_resource(GameSettings {
        movement_speed: 200.0,
        render_distance: 16,
        chunk_spawn_pf: 50,
        mesh_update_pf: 10,
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

fn update(
    mut commands: Commands,
    mut gizmos: Gizmos,
    mut game_info: ResMut<GameInfo>,
    mut perf_ui: Query<&mut Visibility, With<PerfUiEntryFPS>>,
    mut hitboxes: Local<bool>,
    mut debug_menus: Local<Option<bool>>,
    primary_window: Single<&mut Window, With<PrimaryWindow>>,
    game_entities: Query<(Entity, &GameEntity)>,
    chunks: Query<(Entity, &Transform), With<ChunkEntity>>,
    camera: Single<&Transform, With<Camera3d>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    #[cfg(debug_assertions)]
    let debug_menus = debug_menus.get_or_insert(true);
    #[cfg(not(debug_assertions))]
    let debug_menus = debug_menus.get_or_insert(false);

    if keyboard.just_pressed(KeyCode::F3) {
        *debug_menus = !*debug_menus;
    }

    // f3 + h is hard to press on a 60% keyboard so f4 it is
    if keyboard.just_pressed(KeyCode::F4) {
        *hitboxes = !*hitboxes;
    }

    if keyboard.just_pressed(KeyCode::F11) {
        primary_window.into_inner().mode = if primary_window.mode == WindowMode::Windowed {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        }
    }

    // yeah idc
    if keyboard.just_pressed(KeyCode::Digit1) {
        game_info.current_block = Block::STONE;
    } else if keyboard.just_pressed(KeyCode::Digit2) {
        game_info.current_block = Block::DIRT;
    } else if keyboard.just_pressed(KeyCode::Digit3) {
        game_info.current_block = Block::GRASS;
    } else if keyboard.just_pressed(KeyCode::Digit4) {
        game_info.current_block = Block::PLANK;
    } else if keyboard.just_pressed(KeyCode::Digit5) {
        game_info.current_block = Block::BEDROCK;
    } else if keyboard.just_pressed(KeyCode::Digit6) {
        game_info.current_block = Block::SAND;
    } else if keyboard.just_pressed(KeyCode::Digit7) {
        game_info.current_block = Block::WOOD;
    } else if keyboard.just_pressed(KeyCode::Digit8) {
        game_info.current_block = Block::LEAF;
    } else if keyboard.just_pressed(KeyCode::Digit9) {
        game_info.current_block = Block::SNOW;
    }

    for mut visibility in perf_ui.iter_mut() {
        *visibility = if *debug_menus {
            Visibility::Visible
        } else {
            Visibility::Hidden
        }
    }

    if *hitboxes {
        for (_, entity) in game_entities.iter() {
            let mut scale = vec3(1.0, 1.0, 1.0);
            if entity.kind == GameEntityKind::Ferris {
                scale = vec3(1.0, 0.4, 1.0);
            }
            gizmos.cuboid(
                Transform::from_translation(entity.pos + scale / 2.0)
                    .with_scale(scale)
                    .with_rotation(entity.rot),
                Color::srgb(1.0, 1.0, 1.0),
            );
        }
    }

    let ray_origin = camera.translation;
    let ray_direction = camera.forward();
    let max_distance = 5.0;

    if let Some(hit) = ray_cast(
        &game_info,
        ray_origin,
        ray_direction.as_vec3(),
        max_distance,
    ) {
        let hit_global_position = hit.global_position;
        let mut local_pos = hit.local_pos;
        let mut chunk_pos = hit.chunk_pos;

        gizmos.cuboid(
            Transform::from_translation(hit_global_position.as_vec3() + Vec3::splat(0.5)),
            Color::srgb(1.0, 0.0, 0.0),
        );

        if mouse.just_pressed(MouseButton::Left) {
            let mut write_guard = game_info.chunks.write().unwrap();
            if let Some(chunk) = write_guard.get_mut(&chunk_pos) {
                place_block(
                    &mut commands,
                    &game_info,
                    chunk,
                    chunk_pos,
                    &chunks,
                    local_pos,
                    Block::AIR,
                );
            }
        }
        if mouse.just_pressed(MouseButton::Right) {
            let mut write_guard = game_info.chunks.write().unwrap();

            local_pos += hit.normal;

            if local_pos.y >= 0 && local_pos.y < CHUNK_HEIGHT - 1 {
                if local_pos.x < 0 {
                    local_pos.x += CHUNK_SIZE;
                    chunk_pos.x -= 1;
                } else if local_pos.x >= CHUNK_SIZE {
                    local_pos.x -= CHUNK_SIZE;
                    chunk_pos.x += 1;
                }

                if local_pos.z < 0 {
                    local_pos.z += CHUNK_SIZE;
                    chunk_pos.z -= 1;
                } else if local_pos.z >= CHUNK_SIZE {
                    local_pos.z -= CHUNK_SIZE;
                    chunk_pos.z += 1;
                }

                if let Some(chunk) = write_guard.get_mut(&chunk_pos) {
                    if chunk.blocks[vec3_to_index(local_pos)] == Block::AIR {
                        place_block(
                            &mut commands,
                            &game_info,
                            chunk,
                            chunk_pos,
                            &chunks,
                            local_pos,
                            game_info.current_block,
                        );
                    }
                } else {
                    warn!("placing in a chunk that doesn't exist {:?}", chunk_pos);
                }
            }
        }
    }
}

fn handle_chunk_gen(
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

            let Ok(chunks_guard) = game_info.chunks.read() else {
                continue;
            };
            let Ok(loading_chunks_guard) = game_info.loading_chunks.read() else {
                continue;
            };

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
            let saved_chunks_for_task = game_info.saved_chunks.clone();
            let task = thread_pool.spawn(async move {
                let mut chunk = Chunk::new(current_chunk_key);

                for rela_z in 0..CHUNK_SIZE {
                    for rela_x in 0..CHUNK_SIZE {
                        let pos = vec2(
                            (rela_x + chunk_x * CHUNK_SIZE) as f32,
                            (rela_z + chunk_z * CHUNK_SIZE) as f32,
                        );
                        let (max_y, biome) = terrain_noise(pos, seed);

                        for y in 0..CHUNK_HEIGHT {
                            if y == max_y
                                && max_y > SEA_LEVEL
                                && biome < 0.4
                                && ferris_noise(pos, seed) > 0.85
                            {
                                chunk.entities.push((
                                    Entity::PLACEHOLDER,
                                    GameEntity {
                                        kind: GameEntityKind::Ferris,
                                        pos: vec3(pos.x, y as f32, pos.y),
                                        rot: Quat::from_rotation_y(
                                            rand::random_range(0..360) as f32
                                        ),
                                    },
                                ));
                            }
                            chunk.blocks[vec3_to_index(ivec3(rela_x, y, rela_z))] = Block {
                                kind: if y == 0 {
                                    BlockKind::Bedrock
                                } else if y < max_y {
                                    match y {
                                        _ if y > 165 => BlockKind::Snow,
                                        _ if y > 140 => BlockKind::Stone,
                                        _ if y == max_y - 1 => BlockKind::Grass,
                                        _ if y >= max_y - 4 => BlockKind::Dirt,
                                        _ => BlockKind::Stone,
                                    }
                                } else if y < SEA_LEVEL {
                                    BlockKind::Water
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
                                        let mut pos = ivec3(3 + x as i32, y as i32, 3 + z as i32);
                                        let (local_max_y, _) = terrain_noise(
                                            (chunk.pos * CHUNK_SIZE + pos).as_vec3().xz(),
                                            seed,
                                        );

                                        pos.y += local_max_y;

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

                let saved_chunks_guard = saved_chunks_for_task.read().unwrap();
                if let Some(saved_chunk) = saved_chunks_guard.get(&current_chunk_key) {
                    for (pos, block) in &saved_chunk.blocks {
                        chunk.blocks[vec3_to_index(*pos)] = *block;
                    }
                    chunk.entities = saved_chunk.entities.clone();
                }

                (chunk, current_chunk_key)
            });
            commands.spawn(ComputeChunk(task));
        }
    }
}

fn handle_mesh_gen(
    mut commands: Commands,
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
}

fn handle_chunk_despawn(
    mut game_info: ResMut<GameInfo>,
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
    game_info.despawn_tasks.push(task);
}

#[derive(Component)]
struct ComputeChunk(Task<(Chunk, IVec3)>);

#[derive(Component)]
struct ComputeChunkMesh(Task<Option<ChunkMesh>>);

fn process_tasks(
    mut commands: Commands,
    mut mesh_tasks: Query<(Entity, &mut ComputeChunkMesh)>,
    mut spawn_tasks: Query<(Entity, &mut ComputeChunk)>,
    mut game_info: ResMut<GameInfo>,
    mut meshes: ResMut<Assets<Mesh>>,
    game_settings: Res<GameSettings>,
) {
    // SPAWNING CHUNKS
    let mut processed_this_frame = 0;
    for (entity, mut compute_task) in spawn_tasks.iter_mut() {
        if processed_this_frame >= game_settings.chunk_spawn_pf && game_settings.chunk_spawn_pf >= 0
        {
            break;
        }
        if let Some((mut chunk, pos)) = future::block_on(future::poll_once(&mut compute_task.0)) {
            for (entity, game_entity) in chunk.entities.iter_mut() {
                *entity = commands
                    .spawn((
                        *game_entity,
                        SceneRoot(game_info.models[game_entity.kind as usize].clone()),
                        Transform::from_translation(game_entity.pos + vec3(0.5, 0.0, 0.5))
                            .with_scale(Vec3::splat(2.0))
                            .with_rotation(game_entity.rot),
                    ))
                    .id();
            }
            commands
                .entity(entity)
                .try_insert((
                    ChunkEntity,
                    Transform::from_translation((pos * CHUNK_SIZE).as_vec3()),
                ))
                .try_remove::<ComputeChunk>();

            game_info.chunks.write().unwrap().insert(pos, chunk);
            game_info.loading_chunks.write().unwrap().remove(&pos);

            processed_this_frame += 1;
        }
    }

    // GENERATING MESHES
    let mut processed_this_frame = 0;

    for (entity, mut compute_task) in mesh_tasks.iter_mut() {
        if processed_this_frame >= game_settings.mesh_update_pf && game_settings.chunk_spawn_pf >= 0
        {
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

    // DESPAWNING CHUNKS
    let chunks = game_info.chunks.clone();
    let loading_chunks = game_info.loading_chunks.clone();

    game_info.despawn_tasks.retain_mut(|mut compute_task| {
        if let Some(despawn_list) = future::block_on(future::poll_once(&mut compute_task)) {
            let game_chunks_read_guard = chunks.read().unwrap();
            for (_, chunk_key) in despawn_list.iter() {
                let chunk_entities = &game_chunks_read_guard.get(chunk_key).map(|x| &x.entities);
                if let Some(chunk_entities) = chunk_entities {
                    for (entity, _) in chunk_entities.iter() {
                        if *entity != Entity::PLACEHOLDER {
                            commands.entity(*entity).try_despawn();
                        }
                    }
                }
            }
            drop(game_chunks_read_guard);
            let mut game_chunks_write_guard = chunks.write().unwrap();
            let mut game_loading_chunks_write_guard = loading_chunks.write().unwrap();
            for (entity_to_despawn, chunk_key) in despawn_list {
                commands.entity(entity_to_despawn).try_despawn();

                game_chunks_write_guard.remove(&chunk_key);
                game_loading_chunks_write_guard.remove(&chunk_key);
            }
            return false;
        }
        true
    });
}
