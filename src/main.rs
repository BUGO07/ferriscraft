#![allow(clippy::too_many_arguments, clippy::type_complexity)]

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{Arc, RwLock},
};

use bevy::{
    asset::RenderAssetUsages,
    diagnostic::{
        EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
        SystemInformationDiagnosticsPlugin,
    },
    input::{common_conditions::input_just_pressed, mouse::MouseWheel},
    prelude::*,
    render::{
        diagnostic::RenderDiagnosticsPlugin,
        mesh::{Indices, PrimitiveTopology},
        primitives::Aabb,
    },
    tasks::{AsyncComputeTaskPool, Task, futures_lite::future},
    window::{PrimaryWindow, WindowMode},
};
use bevy_inspector_egui::{
    bevy_egui::EguiPlugin,
    quick::{ResourceInspectorPlugin, WorldInspectorPlugin},
};
use bevy_persistent::{Persistent, StorageFormat};
use iyes_perf_ui::{
    PerfUiPlugin,
    prelude::{PerfUiAllEntries, PerfUiEntryFPS},
};

use crate::{
    mesher::{
        Chunk, ChunkEntity, ChunkMesh, GameEntity, GameEntityKind, SavedChunk, SavedWorld,
        build_chunk_mesh,
    },
    player::{
        Player, PlayerCamera, Velocity, camera_movement, player_movement, toggle_grab_cursor,
    },
    utils::{
        Block, BlockKind, TREE_OBJECT, aabb_collision, ferris_noise, get_vertex_u32, place_block,
        ray_cast, terrain_noise, tree_noise, vec3_to_index,
    },
};

pub mod mesher;
pub mod player;
pub mod utils;

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
pub struct PausableSystems;

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
            EguiPlugin::default(),
            WorldInspectorPlugin::default().run_if(
                |perf_ui: Single<&Visibility, With<PerfUiEntryFPS>>| *perf_ui != Visibility::Hidden,
            ),
            ResourceInspectorPlugin::<GameSettings>::default().run_if(
                |perf_ui: Single<&Visibility, With<PerfUiEntryFPS>>| *perf_ui != Visibility::Hidden,
            ),
        ))
        .init_resource::<SavedWorld>()
        .insert_resource(
            Persistent::<SavedWorld>::builder()
                .name("saved world")
                .format(StorageFormat::Bincode)
                .path(Path::new("saves").join("saved_world.ferris"))
                .default(SavedWorld::default())
                .build()
                .unwrap(),
        )
        .configure_sets(
            Update,
            PausableSystems.run_if(|settings: Res<GameSettings>| !settings.paused),
        )
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                toggle_pause.run_if(input_just_pressed(KeyCode::Escape)),
                (
                    player_movement,
                    camera_movement,
                    update,
                    handle_chunk_gen,
                    handle_chunk_despawn
                        .run_if(|game_settings: Res<GameSettings>| game_settings.despawn_chunks),
                    process_tasks,
                    handle_mesh_gen,
                )
                    .in_set(PausableSystems),
            ),
        )
        .run();
}

fn toggle_pause(
    mut game_settings: ResMut<GameSettings>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
) {
    game_settings.paused = !game_settings.paused;
    toggle_grab_cursor(&mut window);
}

#[derive(Resource)]
pub struct GameInfo {
    pub seed: u32,
    pub chunks: Arc<RwLock<HashMap<IVec3, Chunk>>>,
    pub loading_chunks: Arc<RwLock<HashSet<IVec3>>>,
    pub materials: Vec<Handle<StandardMaterial>>,
    pub models: Vec<Handle<Scene>>,
    pub despawn_tasks: Vec<Task<Vec<(Entity, IVec3)>>>,
    pub current_block: Block,
    pub loaded: bool,
}

#[derive(Reflect, Resource, Default)]
pub struct GameSettings {
    pub movement_speed: f32,
    pub sensitivity: f32,
    pub render_distance: i32,
    pub chunk_spawn_pf: i32,
    pub mesh_update_pf: i32,
    pub fov: u32,
    pub gravity: f32,
    pub jump_force: f32,
    pub despawn_chunks: bool,
    pub debug_menus: bool,
    pub hitboxes: bool,
    pub chunk_borders: bool,
    pub paused: bool,
}

pub const CHUNK_SIZE: i32 = 16; // MAX 63
pub const CHUNK_HEIGHT: i32 = 256; // MAX 511
pub const SEA_LEVEL: i32 = 64; // MAX CHUNK_HEIGHT - 180

#[derive(Component)]
pub struct CoordsText;

fn setup(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut saved_chunks: ResMut<SavedWorld>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    persistent_saved_chunks: Res<Persistent<SavedWorld>>,
    asset_server: Res<AssetServer>,
) {
    saved_chunks.0 = persistent_saved_chunks.0;
    saved_chunks.1 = persistent_saved_chunks.1.clone();

    commands.insert_resource(GameInfo {
        seed: saved_chunks.0,
        chunks: Arc::new(RwLock::new(HashMap::new())),
        loading_chunks: Arc::new(RwLock::new(HashSet::new())),
        materials: vec![materials.add(StandardMaterial {
            base_color_texture: Some(asset_server.load("atlas.png")),
            ..default()
        })],
        models: vec![asset_server.load(GltfAssetLabel::Scene(0).from_asset("models/ferris.glb"))],
        despawn_tasks: Vec::new(),
        current_block: Block::STONE,
        loaded: false,
    });
    commands.insert_resource(GameSettings {
        movement_speed: 3.0,
        sensitivity: 1.2,
        fov: 60,
        render_distance: 16,
        chunk_spawn_pf: 50,
        mesh_update_pf: 10,
        gravity: 23.31,
        jump_force: 7.7,
        despawn_chunks: true,
        #[cfg(debug_assertions)]
        debug_menus: true,
        #[cfg(not(debug_assertions))]
        debug_menus: false,
        hitboxes: false,
        chunk_borders: false,
        paused: false,
    });

    let player = commands
        .spawn((
            Transform::from_xyz(
                0.0,
                1.0 + terrain_noise(Vec2::ZERO, saved_chunks.0).0 as f32,
                0.0,
            ),
            Aabb::from_min_max(vec3(-0.25, 0.0, -0.25), vec3(0.25, 1.8, 0.25)),
            Player,
            Velocity::default(),
            Visibility::Visible,
        ))
        .id();

    commands.spawn((
        Camera3d::default(),
        PlayerCamera,
        Transform::from_xyz(0.0, 1.62, -0.05).looking_at(Vec3::ZERO, Vec3::Y), // minecraft way
        ChildOf(player),
    ));

    toggle_grab_cursor(&mut window);

    commands.spawn(PerfUiAllEntries::default());
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::all(Val::Px(5.0)),
                ..default()
            },
            GlobalZIndex(i32::MAX),
        ))
        .with_children(|p| {
            p.spawn(Text::default()).with_children(|p| {
                p.spawn((TextSpan::new("Coords: "), CoordsText));
            });
        });
    // commands.spawn(Text2d::new("hello world"));
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
    mut coords_text: Single<&mut TextSpan, With<CoordsText>>,
    mut persistent_saved_chunks: ResMut<Persistent<SavedWorld>>,
    mut primary_window: Single<&mut Window, With<PrimaryWindow>>,
    mut mouse_scroll: EventReader<MouseWheel>,
    mut game_settings: ResMut<GameSettings>,
    mut camera: Single<(&Transform, &mut Projection), (With<PlayerCamera>, Without<Player>)>,
    player: Single<&Transform, (With<Player>, Without<PlayerCamera>)>,
    saved_chunks: ResMut<SavedWorld>,
    game_entities: Query<(Entity, &GameEntity)>,
    chunks: Query<(Entity, &Transform), With<ChunkEntity>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    for button in keyboard.get_just_pressed() {
        match button {
            KeyCode::F1 => {
                persistent_saved_chunks
                    .update(|sc| {
                        sc.0 = saved_chunks.0;
                        sc.1 = saved_chunks.1.clone()
                    })
                    .unwrap();
            }
            KeyCode::F3 => {
                game_settings.debug_menus = !game_settings.debug_menus;
            }
            // f3 + h is hard to press on a 60% keyboard so f4 it is
            KeyCode::F4 => {
                game_settings.hitboxes = !game_settings.hitboxes;
            }
            KeyCode::F6 => {
                game_settings.chunk_borders = !game_settings.chunk_borders;
            }
            KeyCode::F11 => {
                primary_window.mode = if primary_window.mode == WindowMode::Windowed {
                    WindowMode::BorderlessFullscreen(MonitorSelection::Current)
                } else {
                    WindowMode::Windowed
                }
            }
            KeyCode::Digit1 => game_info.current_block = Block::STONE,
            KeyCode::Digit2 => game_info.current_block = Block::DIRT,
            KeyCode::Digit3 => game_info.current_block = Block::GRASS,
            KeyCode::Digit4 => game_info.current_block = Block::PLANK,
            KeyCode::Digit5 => game_info.current_block = Block::BEDROCK,
            KeyCode::Digit6 => game_info.current_block = Block::SAND,
            KeyCode::Digit7 => game_info.current_block = Block::WOOD,
            KeyCode::Digit8 => game_info.current_block = Block::LEAF,
            KeyCode::Digit9 => game_info.current_block = Block::SNOW,
            _ => {}
        }
    }

    let fov = if keyboard.pressed(KeyCode::KeyC) {
        10.0
    } else {
        game_settings.fov as f32
    };

    *camera.1 = Projection::Perspective(PerspectiveProjection {
        fov: fov.to_radians(),
        ..default()
    });

    for ev in mouse_scroll.read() {
        let dir = ev.y.signum();
        let mut next = game_info.current_block.kind as i32 + dir as i32;
        if next == BlockKind::Water as i32 {
            next += dir as i32;
        }
        if next < 1 {
            next = 10;
        } else if next > 10 {
            next = 1;
        }
        game_info.current_block = Block {
            kind: BlockKind::from_u32(next as u32),
        }
    }

    let (_, biome) = terrain_noise(player.translation.xz(), game_info.seed);
    coords_text.0 = format!(
        "Coord: {:.02}\nBlock: {}\nChunk: {}\nBiome: {}\nIn Hand: {:?}",
        player.translation,
        vec3(
            player.translation.x.rem_euclid(CHUNK_SIZE as f32),
            player.translation.y,
            player.translation.z.rem_euclid(CHUNK_SIZE as f32),
        )
        .as_ivec3(),
        ivec2(
            player.translation.x.div_euclid(CHUNK_SIZE as f32) as i32,
            player.translation.z.div_euclid(CHUNK_SIZE as f32) as i32,
        ),
        // not really
        if biome < 0.4 {
            "Ocean"
        } else if biome > 0.6 {
            "Mountains"
        } else {
            "Plains"
        },
        game_info.current_block.kind
    );

    for mut visibility in perf_ui.iter_mut() {
        *visibility = if game_settings.debug_menus {
            Visibility::Visible
        } else {
            Visibility::Hidden
        }
    }

    if game_settings.hitboxes {
        for (_, entity) in game_entities.iter() {
            let mut scale = vec3(1.0, 1.0, 1.0);
            if entity.kind == GameEntityKind::Ferris {
                scale = vec3(1.0, 0.4, 1.0);
            }
            gizmos.cuboid(
                Transform::from_translation(entity.pos + scale / 2.0)
                    .with_scale(scale)
                    .with_rotation(Quat::from_rotation_y(entity.rot)),
                Color::srgb(1.0, 1.0, 1.0),
            );
        }
    }

    if game_settings.chunk_borders {
        let player = player.translation.floor();
        let chunk_size = CHUNK_SIZE as f32;
        let mut chunk_size_vec = vec2(chunk_size, chunk_size);
        let chunk_pos = vec3(
            player.x.div_euclid(chunk_size) * chunk_size + chunk_size / 2.0,
            CHUNK_HEIGHT as f32 / 2.0,
            player.z.div_euclid(chunk_size) * chunk_size + chunk_size / 2.0,
        );
        for y in (0..CHUNK_HEIGHT).step_by(CHUNK_SIZE as usize) {
            if y == CHUNK_HEIGHT - CHUNK_SIZE {
                chunk_size_vec.y -= 1.0;
            }
            gizmos.rect(
                Isometry3d::from_translation(
                    chunk_pos.with_y(y as f32 + chunk_size_vec.y / 2.0)
                        + Vec3::Z * chunk_size_vec.x / 2.0,
                ),
                chunk_size_vec,
                Color::srgb(0.0, 1.0, 0.0),
            );
            gizmos.rect(
                Isometry3d::from_translation(
                    chunk_pos.with_y(y as f32 + chunk_size_vec.y / 2.0)
                        - Vec3::Z * chunk_size_vec.x / 2.0,
                ),
                chunk_size_vec,
                Color::srgb(0.0, 1.0, 0.0),
            );
            gizmos.rect(
                Isometry3d::new(
                    chunk_pos.with_y(y as f32 + chunk_size_vec.y / 2.0)
                        + Vec3::X * chunk_size_vec.x / 2.0,
                    Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
                ),
                chunk_size_vec,
                Color::srgb(0.0, 1.0, 0.0),
            );
            gizmos.rect(
                Isometry3d::new(
                    chunk_pos.with_y(y as f32 + chunk_size_vec.y / 2.0)
                        - Vec3::X * chunk_size_vec.x / 2.0,
                    Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
                ),
                chunk_size_vec,
                Color::srgb(0.0, 1.0, 0.0),
            );
        }
    }

    if let Some(hit) = ray_cast(
        &game_info,
        player.translation + camera.0.translation,
        camera.0.forward().as_vec3(),
        5.0,
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
                    saved_chunks.into_inner(),
                    chunk,
                    chunk_pos,
                    &chunks,
                    local_pos,
                    Block::AIR,
                );
            }
        } else if mouse.just_pressed(MouseButton::Right) {
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

                if aabb_collision(
                    player.translation,
                    vec3(0.25, 1.8, 0.25),
                    (chunk_pos * CHUNK_SIZE + local_pos).as_vec3(),
                    Vec3::ONE,
                ) {
                    return;
                }

                if let Some(chunk) = write_guard.get_mut(&chunk_pos) {
                    if chunk.blocks[vec3_to_index(local_pos)] == Block::AIR {
                        place_block(
                            &mut commands,
                            saved_chunks.into_inner(),
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
    saved_chunks: Res<SavedWorld>,
    game_info: Res<GameInfo>,
    game_settings: Res<GameSettings>,
    player: Single<&Transform, With<Player>>,
) {
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
            let saved_chunks = saved_chunks.clone();
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
                                        rot: rand::random_range(0..360) as f32,
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

                if let Some(saved_chunk) = saved_chunks.1.get(&current_chunk_key) {
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
    player: Single<&Transform, With<Player>>,
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
    mut saved_chunks: ResMut<SavedWorld>,
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
            if let Some(saved_chunk) = saved_chunks.1.get_mut(&pos) {
                saved_chunk.entities = chunk.entities.clone();
            } else {
                saved_chunks.1.insert(
                    pos,
                    SavedChunk {
                        pos,
                        entities: chunk.entities.clone(),
                        ..default()
                    },
                );
            }
            for (entity, game_entity) in chunk.entities.iter_mut() {
                *entity = commands
                    .spawn((
                        *game_entity,
                        SceneRoot(game_info.models[game_entity.kind as usize].clone()),
                        Transform::from_translation(game_entity.pos + vec3(0.5, 0.0, 0.5))
                            .with_scale(Vec3::splat(2.0))
                            .with_rotation(Quat::from_rotation_y(game_entity.rot)),
                    ))
                    .id();
            }
            commands
                .entity(entity)
                .try_insert((
                    ChunkEntity,
                    Aabb::from_min_max(
                        vec3(0.0, 0.0, 0.0),
                        vec3(CHUNK_SIZE as f32, CHUNK_HEIGHT as f32, CHUNK_SIZE as f32),
                    ),
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

    if !game_info.loaded {
        let read_guard = chunks.read().unwrap();
        if read_guard.len()
            == ((game_settings.render_distance * 2) * (game_settings.render_distance * 2)) as usize
        {
            game_info.loaded = true;
        }
        drop(read_guard);
    }

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
