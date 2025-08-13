#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::match_like_matches_macro,
    clippy::vec_init_then_push,
    clippy::manual_map
)]

use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    path::Path,
    sync::{Arc, RwLock},
};

use bevy::{
    core_pipeline::experimental::taa::TemporalAntiAliasPlugin,
    image::{ImageFilterMode, ImageSamplerDescriptor},
    input::common_conditions::input_just_pressed,
    pbr::wireframe::WireframeConfig,
    prelude::*,
    render::{
        RenderPlugin,
        settings::{RenderCreation, WgpuFeatures, WgpuSettings},
        view::screenshot::{Screenshot, save_to_disk},
    },
    window::{ExitCondition, PresentMode, PrimaryWindow, WindowMode},
};
use bevy_framepace::FramepacePlugin;
use bevy_mod_billboard::plugin::BillboardPlugin;
use ferriscraft::{
    BlockKind, CHUNK_HEIGHT, CHUNK_SIZE, GameEntity, GameEntityKind, Persistent, SavedChunk,
    SavedWorld,
};

use crate::{
    multiplayer::MultiplayerPlugin,
    player::{Player, PlayerPlugin},
    render_pipeline::{PostProcessSettings, RenderPipelinePlugin},
    singleplayer::SinglePlayerPlugin,
    ui::{GameState, MenuState, UIPlugin},
    utils::set_cursor_grab,
    world::{Chunk, WorldPlugin, systems::save_game, utils::NoiseFunctions},
};

mod multiplayer;
mod player;
mod render_pipeline;
mod singleplayer;
mod ui;
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
                    exit_condition: ExitCondition::DontExit,
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
                .set(RenderPlugin {
                    render_creation: RenderCreation::Automatic(WgpuSettings {
                        features: WgpuFeatures::POLYGON_MODE_LINE,
                        ..default()
                    }),
                    ..default()
                }),
            TemporalAntiAliasPlugin,
            FramepacePlugin,
            BillboardPlugin,
        ))
        .add_plugins((
            SinglePlayerPlugin,
            MultiplayerPlugin,
            WorldPlugin,
            PlayerPlugin,
            UIPlugin,
            RenderPipelinePlugin,
        ))
        .insert_resource(AmbientLight {
            brightness: 1000.,
            ..default()
        })
        .init_resource::<GameInfo>()
        .insert_resource(GameSettings {
            render_distance: 16,
            movement_speed: 4.32,
            jump_force: 7.7,
            sensitivity: 1.2,
            fov: 60,
            gravity: -23.31,
            autosave: true,
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
            PausableSystems.run_if(
                |settings: Res<GameSettings>, game_state: Res<State<GameState>>| {
                    !settings.paused || game_state.get() == &GameState::MultiPlayer
                },
            ),
        )
        .configure_sets(
            FixedUpdate,
            PausableSystems.run_if(
                |settings: Res<GameSettings>, game_state: Res<State<GameState>>| {
                    !settings.paused || game_state.get() == &GameState::MultiPlayer
                },
            ),
        )
        .add_systems(Startup, setup)
        // escape button
        .add_systems(
            Update,
            (|mut game_settings: ResMut<GameSettings>,
              mut window: Single<&mut Window, With<PrimaryWindow>>,
              game_state: Res<State<GameState>>,
              menu_state: Res<State<MenuState>>,
              mut next_menu_state: ResMut<NextState<MenuState>>| {
                if game_state.get() == &GameState::Menu {
                    match menu_state.get() {
                        MenuState::Main => {
                            // game_settings.paused = !game_settings.paused;
                            // toggle_grab_cursor(&mut window);
                        }
                        _ => {
                            next_menu_state.set(MenuState::Main);
                        }
                    }
                } else {
                    game_settings.paused = !game_settings.paused;
                    set_cursor_grab(&mut window, !game_settings.paused);
                }
            })
            .run_if(input_just_pressed(KeyCode::Escape)),
        )
        .add_systems(
            Update,
            (
                handle_keybinds.run_if(|settings: Res<GameSettings>| !settings.paused),
                handle_gizmos.in_set(PausableSystems),
            )
                .run_if(not(in_state(GameState::Menu))),
        )
        .run();
}

fn setup(
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut game_info: ResMut<GameInfo>,
    asset_server: Res<AssetServer>,
) {
    let mut mats = Vec::new();
    mats.push(materials.add(StandardMaterial {
        base_color_texture: Some(asset_server.load("atlas.ktx2")),
        reflectance: 0.0,
        ..default()
    }));
    let mut models = Vec::new();
    models.push(asset_server.load(GltfAssetLabel::Scene(0).from_asset("models/ferris.glb")));
    game_info.materials = mats;
    game_info.models = models;
}

#[derive(Resource, Default)]
struct GameInfo {
    chunks: Arc<RwLock<HashMap<IVec3, Chunk>>>,
    loading_chunks: Arc<RwLock<HashSet<IVec3>>>,
    saved_chunks: Option<Arc<RwLock<HashMap<IVec3, SavedChunk>>>>,
    materials: Vec<Handle<StandardMaterial>>,
    models: Vec<Handle<Scene>>,
    noises: NoiseFunctions,
    current_block: BlockKind,
    player_name: String,
    server_addr: Option<SocketAddr>,
    ui_err: Option<String>,
}

#[derive(Reflect, Resource, Default)]
struct GameSettings {
    render_distance: i32,
    movement_speed: f32,
    jump_force: f32,
    sensitivity: f32,
    fov: u32,
    gravity: f32,
    autosave: bool,
    despawn_chunks: bool,
    debug_menus: bool,
    hitboxes: bool,
    chunk_borders: bool,
    paused: bool,
}

fn handle_keybinds(
    mut commands: Commands,
    mut primary_window: Single<&mut Window, With<PrimaryWindow>>,
    mut wireframe_config: ResMut<WireframeConfig>,
    mut game_settings: ResMut<GameSettings>,
    mut game_info: ResMut<GameInfo>,
    mut camera: Single<(&Transform, &mut PostProcessSettings, &mut Projection), With<Camera3d>>,
    persistent_world: Option<ResMut<Persistent<SavedWorld>>>,
    player: Query<(&Transform, &Player)>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    // borrowchecker...
    if keyboard.just_pressed(KeyCode::F1) {
        save_game(persistent_world, player, Some(camera.0), Some(&game_info));
    }
    for button in keyboard.get_just_pressed() {
        match button {
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
                camera.1.sss += 1;
                if camera.1.sss > 8 {
                    camera.1.sss = 0;
                }
            }
            KeyCode::F8 => {
                wireframe_config.global = !wireframe_config.global;
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

    *camera.2 = Projection::Perspective(PerspectiveProjection {
        fov: fov.to_radians(),
        ..default()
    });
}

fn handle_gizmos(
    mut gizmos: Gizmos,
    player: Single<&Transform, With<Player>>,
    game_settings: Res<GameSettings>,
    game_entities: Query<(Entity, &GameEntity)>,
) {
    if game_settings.hitboxes {
        for (_, entity) in game_entities {
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
}
