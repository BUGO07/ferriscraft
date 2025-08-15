use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, RwLock},
};

use bevy::{
    core_pipeline::{Skybox, bloom::Bloom, experimental::taa::TemporalAntiAliasing},
    pbr::ScreenSpaceAmbientOcclusion,
    prelude::*,
    window::PrimaryWindow,
};
use ferriscraft::{BlockKind, Persistent, SavedWorld};
use iyes_perf_ui::prelude::PerfUiAllEntries;

use crate::{
    GameInfo,
    player::{Player, camera_bundle, player_bundle},
    render_pipeline::PostProcessSettings,
    ui::{GameState, coords_bundle, hotbar_block, hotbar_bundle, root_ui_bundle},
    utils::{get_noise_functions, set_cursor_grab},
    world::systems::save_game,
};

pub struct SinglePlayerPlugin;

impl Plugin for SinglePlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::SinglePlayer), setup)
            .add_systems(OnExit(GameState::SinglePlayer), cleanup);
    }
}

#[derive(Resource)]
pub struct SPNewWorld(pub String, pub u32);

#[derive(Resource)]
pub struct SPSavedWorld(pub String);

fn cleanup(
    mut commands: Commands,
    mut game_info: ResMut<GameInfo>,
    persistent_world: Option<ResMut<Persistent<SavedWorld>>>,
    player: Query<(&Transform, &Player)>,
    camera: Single<(Entity, &Transform), With<Camera3d>>,
) {
    save_game(persistent_world, player, Some(camera.1), &game_info);
    commands.remove_resource::<Persistent<SavedWorld>>();
    commands.remove_resource::<SPNewWorld>();
    commands.remove_resource::<SPSavedWorld>();

    game_info.chunks = default();
    game_info.saved_chunks = default();
    game_info.loading_chunks = default();
    commands.entity(camera.0).remove::<(
        TemporalAntiAliasing,
        PostProcessSettings,
        Skybox,
        Bloom,
        ScreenSpaceAmbientOcclusion,
        ChildOf,
    )>();
}

fn setup(
    mut commands: Commands,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut game_info: ResMut<GameInfo>,
    new_world: Option<Res<SPNewWorld>>,
    saved_world: Option<Res<SPSavedWorld>>,
    camera: Single<Entity, With<Camera3d>>,
    asset_server: Res<AssetServer>,
) {
    let persistent = if let Some(new_world) = new_world {
        let SPNewWorld(name, seed) = new_world.into_inner();
        Persistent::<SavedWorld>::new(
            Path::new("saves").join(format!("{}.ferris", name)),
            SavedWorld {
                seed: *seed,
                players: HashMap::new(),
                chunks: HashMap::new(),
            },
            false,
        )
    } else {
        let SPSavedWorld(name) = saved_world.unwrap().into_inner();
        Persistent::<SavedWorld>::new(
            Path::new("saves").join(format!("{}.ferris", name)),
            SavedWorld::default(),
            false,
        )
    };

    let SavedWorld {
        seed,
        players,
        chunks,
    } = &*persistent;

    game_info.noises = get_noise_functions(*seed);
    game_info.saved_chunks = Some(Arc::new(RwLock::new(chunks.clone())));
    game_info.current_block = BlockKind::Stone;

    set_cursor_grab(&mut window, true);

    // godray lights when?
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
        StateScoped(GameState::SinglePlayer),
    ));

    let &(player_pos, player_velocity, player_yaw, player_pitch) = players
        .get(&"Player".to_string())
        .unwrap_or(&(Vec3::INFINITY, Vec3::ZERO, 0.0, 0.0));

    let player = commands
        .spawn(player_bundle(
            player_pos,
            player_velocity,
            player_yaw,
            &game_info.noises,
        ))
        .insert(StateScoped(GameState::SinglePlayer))
        .id();

    commands
        .entity(*camera)
        .remove::<Camera3d>()
        .insert(camera_bundle(
            asset_server.load("skybox.ktx2"),
            player,
            player_pitch,
        ));

    commands
        .spawn(PerfUiAllEntries::default())
        .insert(StateScoped(GameState::SinglePlayer));

    let ui = commands
        .spawn(root_ui_bundle())
        .insert(StateScoped(GameState::SinglePlayer))
        .id();

    commands.spawn(coords_bundle(ui));

    let hotbar = commands.spawn(hotbar_bundle(ui)).id();

    let node = ImageNode::new(asset_server.load("atlas.png"));

    for i in 1..=10 {
        if i == BlockKind::Water as u8 {
            continue;
        }

        commands.spawn(hotbar_block(hotbar, node.clone(), i));
    }

    commands.insert_resource(persistent);
}
