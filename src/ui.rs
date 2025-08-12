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
    window::PrimaryWindow,
};
use bevy_inspector_egui::{
    bevy_egui::EguiPlugin,
    quick::{ResourceInspectorPlugin, WorldInspectorPlugin},
};
use ferriscraft::{BlockKind, DEFAULT_SERVER_PORT};
use iyes_perf_ui::{PerfUiPlugin, prelude::PerfUiEntryFPS};

use crate::{
    CHUNK_SIZE, GameInfo, GameSettings,
    player::Player,
    singleplayer::{SPNewWorld, SPSavedWorld},
    utils::set_cursor_grab,
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
                        if text.0.is_empty() {
                            text.0 = textbox.2.clone();
                        }
                        textbox.0 = false;
                    }
                }
            },
        )
        .add_observer(
            |trigger: Trigger<Pointer<Released>>,
             mut worldsaves: Query<(&mut SavedWorldMarker, Option<&Children>, Entity)>| {
                for (mut marker, children_opt, entity) in worldsaves.iter_mut() {
                    let clicked = trigger.target == entity
                        || children_opt
                            .map(|children| children.iter().any(|c| c == trigger.target))
                            .unwrap_or(false);
                    marker.0 = clicked;
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
        .add_systems(OnEnter(GameState::Menu), ungrab_cursor)
        .add_systems(OnExit(GameState::Menu), grab_cursor)
        .add_systems(Update, (handle_errors, handle_buttons, handle_textboxes).run_if(in_state(GameState::Menu)))
        .add_systems(Update, handle_hud.run_if(not(in_state(GameState::Menu))));
    }
}

fn ungrab_cursor(mut window: Single<&mut Window, With<PrimaryWindow>>) {
    set_cursor_grab(&mut window, false);
}

fn grab_cursor(mut window: Single<&mut Window, With<PrimaryWindow>>) {
    set_cursor_grab(&mut window, true);
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

#[derive(Component)]
pub struct ErrorText;

#[derive(Component)]
pub struct SavedWorldMarker(pub bool);

fn setup(mut commands: Commands, camera: Query<Entity, With<Camera>>) {
    if camera.single().is_err() {
        commands.spawn(Camera3d::default());
    }
}

fn main_menu(mut commands: Commands) {
    let ui = commands
        .spawn(root_ui_bundle())
        .insert(StateScoped(MenuState::Main))
        .id();

    let vertical = commands.spawn(vertical_ui_bundle(ui)).id();

    commands
        .spawn(button("SinglePlayer", vertical, 300.0, 60.0))
        .observe(
            |_trigger: Trigger<Pointer<Released>>, mut state: ResMut<NextState<MenuState>>| {
                state.set(MenuState::SinglePlayer);
            },
        );
    commands
        .spawn(button("MultiPlayer", vertical, 300.0, 60.0))
        .observe(
            |_trigger: Trigger<Pointer<Released>>, mut state: ResMut<NextState<MenuState>>| {
                state.set(MenuState::MultiPlayer);
            },
        );
    commands
        .spawn(button("Quit", vertical, 300.0, 60.0))
        .observe(
            |_trigger: Trigger<Pointer<Released>>, mut app_exit: EventWriter<AppExit>| {
                app_exit.write(AppExit::Success);
            },
        );
}

fn singleplayer_menu(mut commands: Commands) {
    let ui = commands
        .spawn(root_ui_bundle())
        .insert(StateScoped(MenuState::SinglePlayer))
        .id();

    let vertical = commands.spawn(vertical_ui_bundle(ui)).id();

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
                        .spawn(button(&name, vertical, 500.0, 75.0))
                        .insert(SavedWorldMarker(false))
                        .observe(
                            move |trigger: Trigger<Pointer<Pressed>>,
                                  mut commands: Commands,
                                  mut menu_state: ResMut<NextState<MenuState>>,
                                  mut game_state: ResMut<NextState<GameState>>,
                                  buttons: Query<(
                                &SavedWorldMarker,
                                Option<&Children>,
                                Entity,
                            )>| {
                                for (marker, children_opt, entity) in buttons.iter() {
                                    // shit way
                                    let pressed_on = trigger.target == entity
                                        || children_opt
                                            .map(|children| {
                                                children.iter().any(|c| c == trigger.target)
                                            })
                                            .unwrap_or(false);

                                    if marker.0 && pressed_on {
                                        commands.insert_resource(SPSavedWorld(name.clone()));
                                        menu_state.set(MenuState::None);
                                        game_state.set(GameState::SinglePlayer);
                                    }
                                }
                            },
                        );
                    world_count += 1;
                }
            }
        }
    }

    if world_count == 0 {
        commands.spawn((Text::new("No saves found"), ChildOf(vertical)));
    }

    let horizontal = commands.spawn(horizontal_ui_bundle(vertical)).id();

    commands
        .spawn(button("New World", horizontal, 150.0, 50.0))
        .observe(
            |_trigger: Trigger<Pointer<Released>>, mut state: ResMut<NextState<MenuState>>| {
                state.set(MenuState::SinglePlayerNewWorld);
            },
        );
    commands
        .spawn(button("Back", horizontal, 150.0, 50.0))
        .observe(
            |_trigger: Trigger<Pointer<Released>>, mut state: ResMut<NextState<MenuState>>| {
                state.set(MenuState::Main);
            },
        );
}

fn sp_new_world_menu(mut commands: Commands) {
    let ui = commands
        .spawn(root_ui_bundle())
        .insert(StateScoped(MenuState::SinglePlayerNewWorld))
        .id();

    let vertical = commands.spawn(vertical_ui_bundle(ui)).id();

    commands.spawn(text_box("World Name", vertical, 400.0, 60.0));
    commands.spawn(text_box("Seed", vertical, 400.0, 60.0));

    commands.spawn((
        ErrorText,
        Text::new(""),
        TextColor(Color::srgb(1.0, 0.0, 0.0)),
        Node {
            max_width: Val::Px(375.0),
            ..default()
        },
        TextLayout::new_with_justify(JustifyText::Center),
        ChildOf(vertical),
    ));

    let horizontal = commands.spawn(horizontal_ui_bundle(vertical)).id();

    commands
        .spawn(button("Create", horizontal, 150.0, 50.0))
        .observe(
            |_trigger: Trigger<Pointer<Released>>,
             mut commands: Commands,
             mut game_info: ResMut<GameInfo>,
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

                if name.is_empty() {
                    game_info.ui_err = Some("Name cannot be empty".into());
                    return;
                }

                for i in ["/", "\\", ":", "?", "\"", "<", ">", "|"] {
                    if name.contains(i) {
                        game_info.ui_err = Some("Name contains illegal characters".into());
                        return;
                    }
                }

                if name.len() > 20 {
                    game_info.ui_err = Some("Name is too long".into());
                    return;
                }

                if !Path::new("saves").join(format!("{}.ferris", name)).exists() {
                    if seed.is_empty() {
                        game_info.ui_err = None;
                        commands.insert_resource(SPNewWorld(name, rand::random()));
                        menu_state.set(MenuState::None);
                        game_state.set(GameState::SinglePlayer);
                    } else if let Ok(seed) = seed.parse::<u32>() {
                        game_info.ui_err = None;
                        commands.insert_resource(SPNewWorld(name, seed));
                        menu_state.set(MenuState::None);
                        game_state.set(GameState::SinglePlayer);
                    } else {
                        game_info.ui_err = Some("Seed must be a valid number".into());
                    }
                } else {
                    game_info.ui_err = Some("World by that name already exists".into());
                }
            },
        );
    commands
        .spawn(button("Back", horizontal, 150.0, 50.0))
        .observe(
            |_trigger: Trigger<Pointer<Released>>, mut state: ResMut<NextState<MenuState>>| {
                state.set(MenuState::Main);
            },
        );
}

fn multiplayer_menu(mut commands: Commands) {
    let ui = commands
        .spawn(root_ui_bundle())
        .insert(StateScoped(MenuState::MultiPlayer))
        .id();

    let vertical = commands.spawn(vertical_ui_bundle(ui)).id();

    commands.spawn((
        ErrorText,
        Text::new(""),
        TextColor(Color::srgb(1.0, 0.0, 0.0)),
        Node {
            max_width: Val::Px(375.0),
            ..default()
        },
        TextLayout::new_with_justify(JustifyText::Center),
        ChildOf(vertical),
    ));

    commands.spawn(text_box("Player Name", vertical, 400.0, 60.0));
    commands.spawn(text_box("Server IP", vertical, 400.0, 60.0));

    let horizontal = commands.spawn(horizontal_ui_bundle(vertical)).id();

    commands
        .spawn(button("Connect", horizontal, 150.0, 50.0))
        .observe(
            |_trigger: Trigger<Pointer<Released>>,
             mut game_info: ResMut<GameInfo>,
             mut menu_state: ResMut<NextState<MenuState>>,
             mut game_state: ResMut<NextState<GameState>>,
             mut error_text: Single<&mut Text, With<ErrorText>>,
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
                if name.is_empty() {
                    error_text.0 = "Player name cannot be empty".into();
                    return;
                }
                if name.len() > 16 {
                    error_text.0 = "Player name cannot be longer than 16 characters".into();
                    return;
                }
                if ip.is_empty() {
                    error_text.0 = "IP address cannot be empty".into();
                    return;
                }
                if let Ok(addr) = ip.parse::<SocketAddr>() {
                    println!("Connecting to {}", addr);
                    game_info.player_name = name;
                    game_info.server_addr = Some(addr);
                    menu_state.set(MenuState::None);
                    game_state.set(GameState::MultiPlayer);
                } else if let Ok(addr) = ip.parse::<Ipv4Addr>() {
                    println!("Connecting to {}:{}", addr, DEFAULT_SERVER_PORT);
                    game_info.player_name = name;
                    game_info.server_addr =
                        Some(SocketAddr::V4(SocketAddrV4::new(addr, DEFAULT_SERVER_PORT)));
                    menu_state.set(MenuState::None);
                    game_state.set(GameState::MultiPlayer);
                } else {
                    error_text.0 = "Invalid IP address".into();
                }
            },
        );
    commands
        .spawn(button("Back", horizontal, 150.0, 50.0))
        .observe(
            |_trigger: Trigger<Pointer<Released>>, mut state: ResMut<NextState<MenuState>>| {
                state.set(MenuState::Main);
            },
        );
}

fn handle_errors(mut error_text: Single<&mut Text, With<ErrorText>>, game_info: Res<GameInfo>) {
    if game_info.is_changed()
        && let Some(err) = &game_info.ui_err
    {
        error_text.0 = err.clone();
    }
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

    if !game_settings.paused {
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

        for mut visibility in perf_ui {
            *visibility = if game_settings.debug_menus {
                Visibility::Visible
            } else {
                Visibility::Hidden
            }
        }
    }
}

pub fn root_ui_bundle() -> impl Bundle {
    (
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        GlobalZIndex(i32::MAX),
    )
}

pub fn vertical_ui_bundle(parent: Entity) -> impl Bundle {
    (
        Node {
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Percent(1.5),
            ..default()
        },
        ChildOf(parent),
    )
}

pub fn horizontal_ui_bundle(parent: Entity) -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceEvenly,
            flex_direction: FlexDirection::Row,
            column_gap: Val::Percent(3.0),
            ..default()
        },
        ChildOf(parent),
    )
}

pub const NORMAL_BUTTON: Color = Color::srgb(0.15, 0.15, 0.15);
pub const HOVERED_BUTTON: Color = Color::srgb(0.25, 0.25, 0.25);
pub const PRESSED_BUTTON: Color = Color::srgb(0.35, 0.35, 0.35);

pub fn button(text: &str, parent: Entity, size_x: f32, size_y: f32) -> impl Bundle {
    (
        Button,
        Node {
            width: Val::Px(size_x),
            height: Val::Px(size_y),
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
                font_size: (size_x.sqrt() * 1.8).min(26.0),
                ..default()
            },
            TextColor(Color::srgb(0.9, 0.9, 0.9)),
            TextShadow::default(),
        )],
        ChildOf(parent),
    )
}

#[derive(Component)]
pub struct TextBox(pub bool, pub String, pub String);

pub fn text_box(text: &str, parent: Entity, size_x: f32, size_y: f32) -> impl Bundle {
    (
        Button,
        Node {
            width: Val::Px(size_x),
            height: Val::Px(size_y),
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
                font_size: 26.0,
                ..default()
            },
            TextColor(Color::srgb(0.9, 0.9, 0.9)),
            TextShadow::default(),
        )],
        ChildOf(parent),
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
