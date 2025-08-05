use bevy::{
    diagnostic::{
        EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
        SystemInformationDiagnosticsPlugin,
    },
    input::mouse::MouseWheel,
    prelude::*,
    render::diagnostic::RenderDiagnosticsPlugin,
};
use bevy_inspector_egui::{
    bevy_egui::EguiPlugin,
    quick::{ResourceInspectorPlugin, WorldInspectorPlugin},
};
use iyes_perf_ui::{
    PerfUiPlugin,
    prelude::{PerfUiAllEntries, PerfUiEntryFPS},
};

use crate::{
    CHUNK_SIZE, GameInfo, GameSettings, PausableSystems,
    player::Player,
    world::{BlockKind, utils::terrain_noise},
};

pub struct UIPlugin;

impl Plugin for UIPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin,
            RenderDiagnosticsPlugin,
            SystemInformationDiagnosticsPlugin,
            PerfUiPlugin,
            EguiPlugin::default(),
            WorldInspectorPlugin::default()
                .run_if(|game_settings: Res<GameSettings>| game_settings.paused),
            ResourceInspectorPlugin::<GameSettings>::default()
                .run_if(|game_settings: Res<GameSettings>| game_settings.paused),
        ))
        .add_systems(Startup, setup)
        .add_systems(Update, handle_hud.in_set(PausableSystems));
    }
}

#[derive(Component)]
struct CoordsText;

#[derive(Component)]
struct HotbarBlock(u8);

fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.spawn(PerfUiAllEntries::default());
    let ui = commands.spawn(ui_bundle()).id();
    commands.spawn(coords_bundle(ui));

    let hotbar = commands.spawn(hotbar_bundle(ui)).id();

    let node = ImageNode::new(asset_server.load("atlas.png")).with_mode(NodeImageMode::Sliced(
        TextureSlicer {
            border: BorderRect::ZERO,
            center_scale_mode: SliceScaleMode::Stretch,
            sides_scale_mode: SliceScaleMode::Stretch,
            max_corner_scale: 1.0,
        },
    ));

    for i in 1..=10 {
        if i == BlockKind::Water as u8 {
            continue;
        }

        commands.spawn(hotbar_block(hotbar, node.clone(), i));
    }
}

fn handle_hud(
    mut hotbar_blocks: Query<(&mut ImageNode, &HotbarBlock)>,
    mut mouse_scroll: EventReader<MouseWheel>,
    mut game_info: ResMut<GameInfo>,
    mut coords_text: Single<&mut Text, With<CoordsText>>,
    game_settings: Res<GameSettings>,
    player: Single<&Transform, With<Player>>,
    perf_ui: Query<&mut Visibility, With<PerfUiEntryFPS>>,
) {
    for (mut image, block) in hotbar_blocks.iter_mut() {
        if block.0 == game_info.current_block as u8 {
            image.image_mode = NodeImageMode::Sliced(TextureSlicer {
                border: BorderRect::all(2.0),
                ..default()
            });
            image.color = Color::srgb(0.8, 0.8, 0.8);
        } else {
            image.image_mode = NodeImageMode::Auto;
            image.color = Color::WHITE;
        }
    }

    for ev in mouse_scroll.read() {
        let dir = -ev.y.signum();
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

    let (_, biome) = terrain_noise(player.translation.xz(), &game_info.noises);
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

    for mut visibility in perf_ui {
        *visibility = if game_settings.debug_menus {
            Visibility::Visible
        } else {
            Visibility::Hidden
        }
    }
}

fn ui_bundle() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            padding: UiRect::all(Val::Px(5.0)),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        GlobalZIndex(i32::MAX),
    )
}

fn coords_bundle(ui: Entity) -> impl Bundle {
    (
        Text::default(),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(5.0),
            left: Val::Px(5.0),
            ..default()
        },
        CoordsText,
        ChildOf(ui),
    )
}

fn hotbar_bundle(ui: Entity) -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            margin: UiRect::all(Val::Px(5.0)),
            align_items: AlignItems::Center,
            align_content: AlignContent::SpaceEvenly,
            justify_content: JustifyContent::SpaceEvenly,
            width: Val::Px(464.0),
            height: Val::Px(56.0),
            bottom: Val::Vh(2.0),
            ..default()
        },
        BackgroundColor(Color::srgba(0.8, 0.8, 0.8, 0.65)),
        ChildOf(ui),
    )
}

fn hotbar_block(hotbar: Entity, node: ImageNode, idx: u8) -> impl Bundle {
    (
        node.with_rect(Rect::new(
            0.0,
            16.0 * (idx - 1) as f32,
            16.0,
            16.0 * idx as f32,
        )),
        Node {
            width: Val::Px(48.0),
            height: Val::Px(48.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        HotbarBlock(idx),
        ChildOf(hotbar),
    )
}
