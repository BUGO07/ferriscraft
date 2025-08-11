use std::{
    net::{Ipv4Addr, SocketAddr, SocketAddrV4},
    path::Path,
};

use bevy::{
    diagnostic::{
        EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin,
        SystemInformationDiagnosticsPlugin,
    },
    input::{ButtonState, keyboard::KeyboardInput, mouse::MouseWheel},
    prelude::*,
    render::diagnostic::RenderDiagnosticsPlugin,
};
use bevy_inspector_egui::{
    bevy_egui::EguiPlugin,
    quick::{ResourceInspectorPlugin, WorldInspectorPlugin},
};
use ferriscraft::{BlockKind, DEFAULT_SERVER_PORT};
use iyes_perf_ui::{PerfUiPlugin, prelude::PerfUiEntryFPS};

use crate::{
    CHUNK_SIZE, GameInfo, GameSettings, PausableSystems,
    multiplayer::client::MultiplayerMenuInput,
    player::Player,
    singleplayer::{SPNewWorld, SPSavedWorld},
    world::utils::terrain_noise,
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
            WorldInspectorPlugin::default(),
            ResourceInspectorPlugin::<GameSettings>::default(),
        ))
        .add_observer(
            |trigger: Trigger<Pointer<Released>>,
             mut textboxes: Query<(&mut Text, &mut TextBox, &ChildOf, Entity)>| {
                for (mut text, mut textbox, child_of, entity) in textboxes.iter_mut() {
                    if trigger.target == child_of.parent() || trigger.target == entity {
                        if !textbox.0 {
                            if text.0 == textbox.2 {
                                text.0.clear();
                            }
                            text.0.push('|');
                        }
                        textbox.0 = true;
                    } else {
                        if text.0.ends_with("|") {
                            text.0.pop();
                        }
                        textbox.0 = false;
                    }
                }
            },
        )
        .init_state::<GameState>()
        .init_state::<MenuState>()
        .add_systems(Startup, setup)
        .add_systems(OnEnter(MenuState::Main), main_menu)
        .add_systems(OnEnter(MenuState::SinglePlayer), singleplayer_menu)
        .add_systems(OnEnter(MenuState::SinglePlayerNewWorld), sp_new_world_menu)
        .add_systems(OnEnter(MenuState::MultiPlayer), multiplayer_menu)
        .add_systems(Update, (handle_buttons, handle_textboxes))
        .add_systems(Update, handle_hud.in_set(PausableSystems));
    }
}

#[derive(Component)]
struct CoordsText;

#[derive(Component)]
struct HotbarBlock(u8);

#[derive(States, Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
#[states(scoped_entities)]
pub enum GameState {
    #[default]
    Menu,
    SinglePlayer,
    MultiPlayer,
}

#[derive(States, Debug, Clone, Copy, Eq, PartialEq, Hash, Default)]
#[states(scoped_entities)]
pub enum MenuState {
    None,
    #[default]
    Main,
    SinglePlayer,
    SinglePlayerNewWorld,
    MultiPlayer,
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera3d::default());
}

fn main_menu(mut commands: Commands) {
    let ui = commands
        .spawn(ui_bundle())
        .insert(StateScoped(MenuState::Main))
        .id();

    commands.spawn(button("SinglePlayer", ui)).observe(
        |_trigger: Trigger<Pointer<Released>>, mut state: ResMut<NextState<MenuState>>| {
            state.set(MenuState::SinglePlayer);
        },
    );
    commands.spawn(button("MultiPlayer", ui)).observe(
        |_trigger: Trigger<Pointer<Released>>, mut state: ResMut<NextState<MenuState>>| {
            state.set(MenuState::MultiPlayer);
        },
    );
    commands.spawn(button("Quit", ui)).observe(
        |_trigger: Trigger<Pointer<Released>>, mut app_exit: EventWriter<AppExit>| {
            app_exit.write(AppExit::Success);
        },
    );
}

fn singleplayer_menu(mut commands: Commands) {
    let ui = commands
        .spawn(ui_bundle())
        .insert(StateScoped(MenuState::SinglePlayer))
        .id();

    let mut world_count = 0;

    if let Ok(dir) = Path::new("saves").read_dir() {
        for i in dir {
            if let Ok(entry) = i
                && let Ok(ft) = entry.file_type()
                && ft.is_file()
            {
                let mut name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".ferris") {
                    name = name.replace(".ferris", "");
                    commands
                    .spawn(button(&name, ui))
                    .observe(
                        move |_trigger: Trigger<Pointer<Released>>,
                              mut commands: Commands,
                              mut menu_state: ResMut<NextState<MenuState>>,
                              mut game_state: ResMut<NextState<GameState>>| {
                            commands.insert_resource(SPSavedWorld(name.clone()));
                            menu_state.set(MenuState::None);
                            game_state.set(GameState::SinglePlayer);
                        },
                    );
                    world_count += 1;
                }
            }
        }
    }

    if world_count == 0 {
        commands.spawn((Text::new("No saves found"), ChildOf(ui)));
    }

    commands.spawn(button("Create New", ui)).observe(
        |_trigger: Trigger<Pointer<Released>>, mut state: ResMut<NextState<MenuState>>| {
            state.set(MenuState::SinglePlayerNewWorld);
        },
    );
}

fn sp_new_world_menu(mut commands: Commands) {
    let ui = commands
        .spawn(ui_bundle())
        .insert(StateScoped(MenuState::SinglePlayerNewWorld))
        .id();

    commands.spawn(text_box("World Name", ui));
    commands.spawn(text_box("Seed", ui));
    commands.spawn(button("Create", ui)).observe(
        |_trigger: Trigger<Pointer<Released>>,
         mut commands: Commands,
         mut menu_state: ResMut<NextState<MenuState>>,
         mut game_state: ResMut<NextState<GameState>>,
         textbox: Query<&mut TextBox>| {
            let mut name = String::new();
            let mut seed = String::new();
            for t in textbox.iter() {
                if t.2 == "World Name" {
                    name = t.1.clone();
                }
                if t.2 == "Seed" {
                    seed = t.1.clone();
                }
            }
            if !name.is_empty() && !Path::new("saves").join(format!("{}.ferris", name)).exists() {
                if seed.is_empty() {
                    commands.insert_resource(SPNewWorld(name, rand::random()));
                    menu_state.set(MenuState::None);
                    game_state.set(GameState::SinglePlayer);
                } else if let Ok(seed) = seed.parse::<u32>() {
                    commands.insert_resource(SPNewWorld(name, seed));
                    menu_state.set(MenuState::None);
                    game_state.set(GameState::SinglePlayer);
                } else {
                    println!("Seed must be a valid number");
                }
            } else {
                println!("World by the name {} already exists", name);
            }
        },
    );
    commands.spawn(button("Back", ui)).observe(
        |_trigger: Trigger<Pointer<Released>>, mut state: ResMut<NextState<MenuState>>| {
            state.set(MenuState::Main);
        },
    );
}

fn multiplayer_menu(mut commands: Commands) {
    let ui = commands
        .spawn(ui_bundle())
        .insert(StateScoped(MenuState::MultiPlayer))
        .id();

    commands.spawn(text_box("Player Name", ui));
    commands.spawn(text_box("Server IP", ui));
    commands.spawn(button("Connect", ui)).observe(
        |_trigger: Trigger<Pointer<Released>>,
         mut commands: Commands,
         mut menu_state: ResMut<NextState<MenuState>>,
         mut game_state: ResMut<NextState<GameState>>,
         textbox: Query<&mut TextBox>| {
            let mut name = String::new();
            let mut ip = String::new();
            for t in textbox.iter() {
                if t.2 == "Player Name" {
                    name = t.1.clone();
                }
                if t.2 == "Server IP" {
                    ip = t.1.clone();
                }
            }
            if let Ok(addr) = ip.parse::<SocketAddr>() {
                println!("Connecting to {}", addr);
                commands.insert_resource(MultiplayerMenuInput(addr, name));
                menu_state.set(MenuState::None);
                game_state.set(GameState::MultiPlayer);
            } else if let Ok(addr) = ip.parse::<Ipv4Addr>() {
                println!("Connecting to {}", addr);
                commands.insert_resource(MultiplayerMenuInput(
                    SocketAddr::V4(SocketAddrV4::new(addr, DEFAULT_SERVER_PORT)),
                    name,
                ));
                menu_state.set(MenuState::None);
                game_state.set(GameState::MultiPlayer);
            } else {
                println!("Invalid IP address");
            }
        },
    );
    commands.spawn(button("Back", ui)).observe(
        |_trigger: Trigger<Pointer<Released>>, mut state: ResMut<NextState<MenuState>>| {
            state.set(MenuState::Main);
        },
    );
}

fn handle_buttons(
    mut interaction_query: Query<
        (&Interaction, &mut BackgroundColor),
        (Changed<Interaction>, Or<(With<Button>, With<TextBox>)>),
    >,
) {
    for (interaction, mut color) in &mut interaction_query {
        match *interaction {
            Interaction::Pressed => {
                color.0 = PRESSED_BUTTON;
            }
            Interaction::Hovered => {
                color.0 = HOVERED_BUTTON;
            }
            Interaction::None => {
                color.0 = NORMAL_BUTTON;
            }
        }
    }
}

fn handle_textboxes(
    mut key_evr: EventReader<KeyboardInput>,
    mut query: Query<(&mut Text, &mut TextBox)>,
) {
    for (mut text, mut textbox) in query.iter_mut() {
        if textbox.0 {
            for ev in key_evr.read() {
                if ev.state == ButtonState::Pressed {
                    if ev.key_code == KeyCode::Backspace {
                        textbox.1.pop();
                    } else if let Some(t) = &ev.text {
                        for ch in t.chars() {
                            textbox.1.push(ch);
                        }
                    }
                }
            }
            text.0 = textbox.1.clone();
            if textbox.0 {
                text.0.push('|');
            }
            if text.0.is_empty() {
                text.0 = textbox.2.clone();
            }
        }
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

    let deg = player.rotation.to_euler(EulerRot::YXZ).0.to_degrees();
    let deg = if deg < 0.0 { deg + 360.0 } else { deg };
    coords_text.0 = format!(
        "Coord: {:.02}\nBlock: {}\nChunk: {}\nBiome: {}\nFacing: {} - {}deg\nIn Hand: {:?}",
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
        match deg {
            x if !(22.5..337.5).contains(&x) => "N",
            x if (22.5..67.5).contains(&x) => "NE",
            x if (67.5..112.5).contains(&x) => "E",
            x if (112.5..157.5).contains(&x) => "SE",
            x if (157.5..202.5).contains(&x) => "S",
            x if (202.5..247.5).contains(&x) => "SW",
            x if (247.5..292.5).contains(&x) => "W",
            x if (292.5..337.5).contains(&x) => "NW",
            _ => "N",
        },
        deg as i32,
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

pub fn ui_bundle() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Percent(1.0),
            ..default()
        },
        GlobalZIndex(i32::MAX),
    )
}

pub const NORMAL_BUTTON: Color = Color::srgb(0.15, 0.15, 0.15);
pub const HOVERED_BUTTON: Color = Color::srgb(0.25, 0.25, 0.25);
pub const PRESSED_BUTTON: Color = Color::srgb(0.35, 0.35, 0.35);

pub fn button(text: &str, ui: Entity) -> impl Bundle {
    (
        Button,
        Node {
            width: Val::Px(300.0),
            height: Val::Px(60.0),
            border: UiRect::all(Val::Px(5.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BorderColor(Color::BLACK),
        BorderRadius::MAX,
        BackgroundColor(NORMAL_BUTTON),
        children![(
            Text::new(text),
            TextFont {
                // font: asset_server.load("fonts/FiraSans-Bold.ttf"),
                font_size: 33.0,
                ..default()
            },
            TextColor(Color::srgb(0.9, 0.9, 0.9)),
            TextShadow::default(),
        )],
        ChildOf(ui),
    )
}

#[derive(Component)]
pub struct TextBox(pub bool, pub String, pub String);

pub fn text_box(text: &str, ui: Entity) -> impl Bundle {
    (
        Button,
        Node {
            width: Val::Px(300.0),
            height: Val::Px(60.0),
            border: UiRect::all(Val::Px(5.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BorderColor(Color::BLACK),
        BorderRadius::MAX,
        BackgroundColor(NORMAL_BUTTON),
        children![(
            TextBox(false, String::new(), text.into()),
            Text::new(text),
            TextFont {
                // font: asset_server.load("fonts/FiraSans-Bold.ttf"),
                font_size: 33.0,
                ..default()
            },
            TextColor(Color::srgb(0.9, 0.9, 0.9)),
            TextShadow::default(),
        )],
        ChildOf(ui),
    )
}

pub fn coords_bundle(ui: Entity) -> impl Bundle {
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

pub fn hotbar_bundle(ui: Entity) -> impl Bundle {
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

pub fn hotbar_block(hotbar: Entity, node: ImageNode, idx: u8) -> impl Bundle {
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
