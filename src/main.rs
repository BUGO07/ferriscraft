#![allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::match_like_matches_macro
)]

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{Arc, RwLock},
};

use bevy::{
    diagnostic::{
        EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
        SystemInformationDiagnosticsPlugin,
    },
    image::{ImageFilterMode, ImageSamplerDescriptor},
    input::{common_conditions::input_just_pressed, mouse::MouseWheel},
    prelude::*,
    render::{
        diagnostic::RenderDiagnosticsPlugin,
        primitives::Aabb,
        view::screenshot::{Screenshot, save_to_disk},
    },
    window::{PresentMode, PrimaryWindow, WindowMode},
};
use bevy_framepace::FramepacePlugin;
use bevy_inspector_egui::{
    bevy_egui::EguiPlugin,
    quick::{ResourceInspectorPlugin, WorldInspectorPlugin},
};
use bevy_persistent::Persistent;
use iyes_perf_ui::{
    PerfUiPlugin,
    prelude::{PerfUiAllEntries, PerfUiEntryFPS},
};

use crate::{
    player::{Player, PlayerCamera, PlayerPlugin, Velocity},
    render_pipeline::{PostProcessSettings, RenderPipelinePlugin, VoxelMaterial},
    utils::{
        aabb_collision, place_block, ray_cast, terrain_noise, toggle_grab_cursor, vec3_to_index,
    },
    world::{
        Block, BlockKind, Chunk, ChunkMarker, GameEntity, GameEntityKind, SavedChunk, SavedWorld,
        WorldPlugin,
    },
};

mod mesher;
mod player;
mod render_pipeline;
mod utils;
mod world;

#[derive(SystemSet, Debug, Hash, PartialEq, Eq, Clone)]
struct PausableSystems;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "FerrisCraft".to_string(),
                        mode: WindowMode::Windowed,
                        present_mode: PresentMode::AutoNoVsync,
                        ..default()
                    }),
                    ..default()
                })
                .set(ImagePlugin {
                    // for low res textures
                    default_sampler: ImageSamplerDescriptor {
                        min_filter: ImageFilterMode::Nearest,
                        mag_filter: ImageFilterMode::Nearest,
                        mipmap_filter: ImageFilterMode::Nearest,
                        ..default()
                    },
                })
                .set(AssetPlugin {
                    // for messing with shaders without restarting the game
                    watch_for_changes_override: Some(true),
                    ..default()
                }),
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin,
            RenderDiagnosticsPlugin,
            SystemInformationDiagnosticsPlugin,
            FramepacePlugin,
            PerfUiPlugin,
            EguiPlugin::default(),
            WorldInspectorPlugin::default().run_if(
                |perf_ui: Single<&Visibility, With<PerfUiEntryFPS>>| *perf_ui != Visibility::Hidden,
            ),
            ResourceInspectorPlugin::<GameSettings>::default().run_if(
                |perf_ui: Single<&Visibility, With<PerfUiEntryFPS>>| *perf_ui != Visibility::Hidden,
            ),
        ))
        .add_plugins((WorldPlugin, PlayerPlugin, RenderPipelinePlugin))
        .init_resource::<GameInfo>()
        .insert_resource(GameSettings {
            movement_speed: 3.0,
            sensitivity: 1.2,
            fov: 60,
            render_distance: 16,
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
        })
        .configure_sets(
            Update,
            PausableSystems.run_if(|settings: Res<GameSettings>| !settings.paused),
        )
        .add_systems(Startup, setup)
        .add_systems(Update, update.in_set(PausableSystems))
        // toggle pause
        .add_systems(
            Update,
            (|mut game_settings: ResMut<GameSettings>,
              mut window: Single<&mut Window, With<PrimaryWindow>>| {
                game_settings.paused = !game_settings.paused;
                toggle_grab_cursor(&mut window);
            })
            .run_if(input_just_pressed(KeyCode::Escape)),
        )
        .run();
}

const CHUNK_SIZE: i32 = 16; // MAX 63
const CHUNK_HEIGHT: i32 = 256; // MAX 511
const SEA_LEVEL: i32 = 64; // MAX CHUNK_HEIGHT - 180

#[derive(Resource, Default)]
struct GameInfo {
    seed: u32,
    chunks: Arc<RwLock<HashMap<IVec3, Chunk>>>,
    loading_chunks: Arc<RwLock<HashSet<IVec3>>>,
    saved_chunks: Arc<RwLock<HashMap<IVec3, SavedChunk>>>,
    materials: Vec<Handle<VoxelMaterial>>,
    models: Vec<Handle<Scene>>,
    current_block: BlockKind,
}

#[derive(Reflect, Resource, Default)]
struct GameSettings {
    movement_speed: f32,
    sensitivity: f32,
    render_distance: i32,
    fov: u32,
    gravity: f32,
    jump_force: f32,
    despawn_chunks: bool,
    debug_menus: bool,
    hitboxes: bool,
    chunk_borders: bool,
    paused: bool,
}

#[derive(Component)]
struct HUDText;

fn setup(
    mut commands: Commands,
    mut materials: ResMut<Assets<VoxelMaterial>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut game_info: ResMut<GameInfo>,
    persistent_saved_chunks: Res<Persistent<SavedWorld>>,
    asset_server: Res<AssetServer>,
) {
    let seed = persistent_saved_chunks.0;

    game_info.seed = seed;
    game_info.saved_chunks = Arc::new(RwLock::new(persistent_saved_chunks.1.clone()));
    game_info.materials.push(materials.add(VoxelMaterial {
        color_texture: Some(asset_server.load("atlas.ktx2")),
    }));
    game_info
        .models
        .push(asset_server.load(GltfAssetLabel::Scene(0).from_asset("models/ferris.glb")));
    game_info.current_block = BlockKind::Stone;

    toggle_grab_cursor(&mut window);

    commands.spawn(PerfUiAllEntries::default());

    commands.spawn((
        DirectionalLight {
            illuminance: 5_000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(
            EulerRot::ZYX,
            0.0,
            33.5_f32.to_radians(),
            -47.3_f32.to_radians(),
        )),
    ));

    let player = commands
        .spawn((
            Transform::from_xyz(0.0, 1.0 + terrain_noise(Vec2::ZERO, seed).0 as f32, 0.0),
            Aabb::from_min_max(vec3(-0.25, 0.0, -0.25), vec3(0.25, 1.8, 0.25)),
            Player,
            Velocity::default(),
            Visibility::Visible,
        ))
        .id();

    commands.spawn((
        Camera3d::default(),
        PlayerCamera,
        PostProcessSettings::default(),
        Transform::from_xyz(0.0, 1.62, -0.05).looking_at(Vec3::ZERO, Vec3::Y), // minecraft way
        ChildOf(player),
    ));

    let ui = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                padding: UiRect::all(Val::Px(5.0)),
                ..default()
            },
            GlobalZIndex(i32::MAX),
        ))
        .id();

    commands.spawn((Text::default(), HUDText, ChildOf(ui)));
}

fn update(
    mut commands: Commands,
    mut gizmos: Gizmos,
    mut game_info: ResMut<GameInfo>,
    mut perf_ui: Query<&mut Visibility, With<PerfUiEntryFPS>>,
    mut coords_text: Single<&mut Text, With<HUDText>>,
    mut persistent_saved_chunks: ResMut<Persistent<SavedWorld>>,
    mut primary_window: Single<&mut Window, With<PrimaryWindow>>,
    mut mouse_scroll: EventReader<MouseWheel>,
    mut game_settings: ResMut<GameSettings>,
    mut camera: Single<(&Transform, &mut Projection, &mut PostProcessSettings)>,
    player: Single<&Transform, With<Player>>,
    game_entities: Query<(Entity, &GameEntity)>,
    chunks: Query<(Entity, &Transform), With<ChunkMarker>>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    for button in keyboard.get_just_pressed() {
        match button {
            KeyCode::F1 => {
                persistent_saved_chunks
                    .update(|sc| {
                        let saved_chunks = game_info.saved_chunks.read().unwrap();
                        sc.0 = game_info.seed;
                        sc.1 = saved_chunks.clone()
                    })
                    .unwrap();
            }
            KeyCode::F2 => {
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(Path::new("screenshots").join(format!(
                        "screenshot-{}.png",
                        chrono::Local::now().format("%Y-%m-%d-%H-%M-%S%.3fZ")
                    ))));
            }
            KeyCode::F3 => {
                game_settings.debug_menus = !game_settings.debug_menus;
            }
            KeyCode::F4 => {
                game_settings.hitboxes = !game_settings.hitboxes;
            }
            KeyCode::F6 => {
                game_settings.chunk_borders = !game_settings.chunk_borders;
            }
            KeyCode::F7 => {
                camera.2.sss += 1;
                if camera.2.sss > 8 {
                    camera.2.sss = 0;
                }
            }
            KeyCode::F11 => {
                primary_window.mode = if primary_window.mode == WindowMode::Windowed {
                    WindowMode::BorderlessFullscreen(MonitorSelection::Current)
                } else {
                    WindowMode::Windowed
                }
            }
            KeyCode::Digit1 => game_info.current_block = BlockKind::Stone,
            KeyCode::Digit2 => game_info.current_block = BlockKind::Dirt,
            KeyCode::Digit3 => game_info.current_block = BlockKind::Grass,
            KeyCode::Digit4 => game_info.current_block = BlockKind::Plank,
            KeyCode::Digit5 => game_info.current_block = BlockKind::Bedrock,
            KeyCode::Digit6 => game_info.current_block = BlockKind::Sand,
            KeyCode::Digit7 => game_info.current_block = BlockKind::Wood,
            KeyCode::Digit8 => game_info.current_block = BlockKind::Leaf,
            KeyCode::Digit9 => game_info.current_block = BlockKind::Snow,
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
        let mut next = game_info.current_block as i32 + dir as i32;
        if next == BlockKind::Water as i32 {
            next += dir as i32;
        }
        if next < 1 {
            next = 10;
        } else if next > 10 {
            next = 1;
        }
        game_info.current_block = BlockKind::from_u32(next as u32);
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
        game_info.current_block
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
                    &mut game_info.saved_chunks.write().unwrap(),
                    chunk,
                    &chunks,
                    local_pos,
                    Block::AIR,
                );
            }
        } else if mouse.just_pressed(MouseButton::Right) {
            let mut write_guard = game_info.chunks.write().unwrap();

            local_pos += hit.normal.as_vec3().as_ivec3();

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
                    hit_global_position.as_vec3() + hit.normal.as_vec3(),
                    Vec3::ONE,
                ) {
                    return;
                }

                if let Some(chunk) = write_guard.get_mut(&chunk_pos) {
                    if chunk.blocks[vec3_to_index(local_pos)] == Block::AIR {
                        place_block(
                            &mut commands,
                            &mut game_info.saved_chunks.write().unwrap(),
                            chunk,
                            &chunks,
                            local_pos,
                            Block {
                                kind: game_info.current_block,
                                direction: if game_info.current_block.can_rotate() {
                                    hit.normal
                                } else {
                                    Default::default()
                                },
                            },
                        );
                    }
                } else {
                    warn!("placing in a chunk that doesn't exist {:?}", chunk_pos);
                }
            }
        }
    }
}
