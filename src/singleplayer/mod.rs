use std::{collections::HashMap, path::Path};

use bevy::{prelude::*, window::PrimaryWindow};
use bevy_persistent::{Persistent, StorageFormat};
use ferriscraft::{BlockKind, SavedWorld};
use iyes_perf_ui::prelude::PerfUiAllEntries;

use crate::{
    GameInfo,
    player::{camera_bundle, player_bundle},
    ui::{GameState, coords_bundle, hotbar_block, hotbar_bundle, ui_bundle},
    utils::{get_noise_functions, toggle_grab_cursor},
};

pub struct SinglePlayerPlugin;

impl Plugin for SinglePlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::SinglePlayer), setup);
    }
}

#[derive(Resource)]
pub struct SPNewWorld(pub String, pub u32);

#[derive(Resource)]
pub struct SPSavedWorld(pub String);

fn setup(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    new_world: Option<Res<SPNewWorld>>,
    saved_world: Option<Res<SPSavedWorld>>,
    camera: Single<(Entity, &mut Camera3d)>,
    asset_server: Res<AssetServer>,
) {
    let persistent = if let Some(new_world) = new_world {
        let SPNewWorld(name, seed) = new_world.into_inner();
        Persistent::<SavedWorld>::builder()
                .name(name)
                .format(StorageFormat::Bincode)
                .path(Path::new("saves").join(format!("{}.ferris", name)))
                .default(SavedWorld(
                    *seed,
                    HashMap::new(),
                    HashMap::new(),
                ))
                .build()
                .expect("World save couldn't be read, please make a backup of saves/world.ferris and remove it from the saves folder.")
    } else {
        let SPSavedWorld(name) = saved_world.unwrap().into_inner();
        Persistent::<SavedWorld>::builder()
                .name(name)
                .format(StorageFormat::Bincode)
                .path(Path::new("saves").join(format!("{}.ferris", name)))
                .default(SavedWorld::default())
                .build()
                .expect("World save couldn't be read, please make a backup of saves/world.ferris and remove it from the saves folder.")
    };

    let mut mats = Vec::new();
    mats.push(materials.add(StandardMaterial {
        base_color_texture: Some(asset_server.load("atlas.ktx2")),
        reflectance: 0.0,
        ..default()
    }));
    let mut models = Vec::new();
    models.push(asset_server.load(GltfAssetLabel::Scene(0).from_asset("models/ferris.glb")));

    let game_info = GameInfo {
        noises: get_noise_functions(persistent.0),
        materials: mats,
        models,
        current_block: BlockKind::Stone,
        ..default()
    };

    toggle_grab_cursor(&mut window);

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

    let player_velocity = Vec3::ZERO;
    let player_yaw = 0.0;
    let player_pitch = 0.0;

    let player = commands
        .spawn(player_bundle(
            persistent
                .1
                .iter()
                .next()
                .unwrap_or((
                    &"Player".to_string(),
                    &(Vec3::INFINITY, Vec3::ZERO, 0.0, 0.0),
                ))
                .1
                .0,
            player_velocity,
            player_yaw,
            &game_info.noises,
        ))
        .insert(StateScoped(GameState::SinglePlayer))
        .id();

    commands
        .entity(camera.0)
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
        .spawn(ui_bundle())
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

    commands.insert_resource(game_info);
}
