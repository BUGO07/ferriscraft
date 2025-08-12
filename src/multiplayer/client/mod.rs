use std::{collections::HashSet, net::UdpSocket, time::SystemTime};

use bevy::{
    core_pipeline::{Skybox, bloom::Bloom, experimental::taa::TemporalAntiAliasing},
    pbr::ScreenSpaceAmbientOcclusion,
    prelude::*,
    window::PrimaryWindow,
};
use bevy_mod_billboard::BillboardText;
use bevy_renet::{
    RenetClientPlugin,
    netcode::{
        ClientAuthentication, NETCODE_USER_DATA_BYTES, NetcodeClientPlugin, NetcodeClientTransport,
    },
    renet::{ConnectionConfig, DefaultChannel, DisconnectReason, RenetClient},
};
use ferriscraft::{BlockKind, CHUNK_SIZE, ClientPacket, ServerPacket};
use iyes_perf_ui::prelude::PerfUiAllEntries;

use crate::{
    GameInfo,
    player::{OnlinePlayer, PlayerCamera, camera_bundle, player_bundle},
    render_pipeline::PostProcessSettings,
    ui::{GameState, MenuState, coords_bundle, hotbar_block, hotbar_bundle, root_ui_bundle},
    utils::{get_noise_functions, set_cursor_grab},
    world::{
        ChunkMarker,
        utils::{place_block, update_chunks},
    },
};

pub struct MultiplayerPlugin;

impl Plugin for MultiplayerPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.add_plugins((RenetClientPlugin, NetcodeClientPlugin))
            .add_event::<ClientEvent>()
            .add_systems(OnEnter(GameState::MultiPlayer), setup)
            .add_systems(
                Update,
                (client_event_handler, send_client_data, receive_server_data)
                    .run_if(in_state(GameState::MultiPlayer)),
            );
    }
}

fn setup(mut commands: Commands, multiplayer_input: Res<GameInfo>) {
    let current_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("system clock is wrong");

    let mut user_data = [0; NETCODE_USER_DATA_BYTES];
    // let name = std::env::args()
    //     .nth(1)
    //     .unwrap_or(format!("Player {}", rand::random_range(0..1000)));
    let bytes = multiplayer_input.player_name.as_bytes();
    user_data[..bytes.len()].copy_from_slice(bytes);
    commands.remove_resource::<RenetClient>();
    commands.remove_resource::<NetcodeClientTransport>();
    commands.insert_resource(RenetClient::new(ConnectionConfig::default()));

    let version = env!("CARGO_PKG_VERSION").split(".").collect::<Vec<_>>();

    commands.insert_resource(
        NetcodeClientTransport::new(
            current_time,
            ClientAuthentication::Unsecure {
                server_addr: multiplayer_input.server_addr.unwrap(),
                client_id: current_time.as_millis() as u64,
                user_data: Some(user_data),
                protocol_id: version[0].parse::<u64>().unwrap() * 1_000_000
                    + version[1].parse::<u64>().unwrap() * 1_000
                    + version[2].parse::<u64>().unwrap(),
            },
            UdpSocket::bind("0.0.0.0:0").unwrap(),
        )
        .unwrap(),
    );
}

#[derive(Event)]
pub enum ClientEvent {
    Connected(u32, Vec3), // seed, pos
    Disconnected(DisconnectReason),
}

fn client_event_handler(
    mut commands: Commands,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut client_events: EventReader<ClientEvent>,
    // persistent_world: Res<Persistent<SavedWorld>>,
    mut game_info: ResMut<GameInfo>,
    mut game_state: ResMut<NextState<GameState>>,
    mut menu_state: ResMut<NextState<MenuState>>,
    camera: Single<(Entity, &mut Camera3d)>,
    asset_server: Res<AssetServer>,
) {
    for event in client_events.read() {
        match event {
            ClientEvent::Disconnected(_reason) => {
                info!("Disconnected from the server");
                game_info.server_addr = None;
                game_info.player_name = "Player".to_string();
                game_info.chunks = default();
                game_info.saved_chunks = default();
                game_info.loading_chunks = default();
                // idfk it doesnt properly work without doing this
                commands.entity(camera.0).remove::<(
                    TemporalAntiAliasing,
                    PostProcessSettings,
                    Skybox,
                    Bloom,
                    ScreenSpaceAmbientOcclusion,
                    PlayerCamera,
                    ChildOf,
                )>();
                game_state.set(GameState::Menu);
                menu_state.set(MenuState::MultiPlayer);
            }
            &ClientEvent::Connected(seed, pos) => {
                info!("Connected to server");

                game_info.noises = get_noise_functions(seed);
                game_info.current_block = BlockKind::Stone;
                game_info.chunks = default();
                game_info.saved_chunks = default();
                game_info.loading_chunks = default();

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
                    StateScoped(GameState::MultiPlayer),
                ));

                let player_velocity = Vec3::ZERO;
                let player_yaw = 0.0;
                let player_pitch = 0.0;

                let player = commands
                    .spawn(player_bundle(
                        pos,
                        player_velocity,
                        player_yaw,
                        &game_info.noises,
                    ))
                    .insert(StateScoped(GameState::MultiPlayer))
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
                    .insert(StateScoped(GameState::MultiPlayer));

                let ui = commands
                    .spawn(root_ui_bundle())
                    .insert(StateScoped(GameState::MultiPlayer))
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
            }
        }
    }
    // let &SavedWorld(seed, _, ref saved_chunks) = persistent_world.get();
}

fn send_client_data(
    mut client_event: EventWriter<ClientEvent>,
    client: ResMut<RenetClient>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if client.is_disconnected() {
        client_event.write(ClientEvent::Disconnected(
            client.disconnect_reason().unwrap(),
        ));
    }
    if keyboard.just_pressed(KeyCode::KeyT) {
        ClientPacket::ChatMessage("shice".into()).send(Some(client));
    }
}

fn receive_server_data(
    mut commands: Commands,
    mut client: ResMut<RenetClient>,
    mut players: Query<(Entity, &mut Transform, &OnlinePlayer), Without<ChunkMarker>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut client_event: EventWriter<ClientEvent>,
    chunks: Query<(Entity, &Transform), With<ChunkMarker>>,
    game_info: Res<GameInfo>,
    // transport: Res<NetcodeClientTransport>,
) {
    while let Some(message) = client.receive_message(DefaultChannel::ReliableOrdered) {
        let Ok(packet) = bincode::deserialize(&message) else {
            continue;
        };
        match packet {
            ServerPacket::ChatMessage(player, message) => {
                println!("[{player}] {message}");
            }
            ServerPacket::PlayerConnected(player, _pos) => {
                println!("{player} joined the server");
                // if id != transport.client_id() {

                // }
            }
            ServerPacket::PlayerDisconnected(player, reason) => {
                if let Some((entity, _, _)) =
                    players.iter_mut().find(|(_, _, other)| other.0 == player)
                {
                    commands.entity(entity).despawn();
                }
                println!("{player} left the server: {reason}");
            }
            ServerPacket::ConnectionInfo(seed, pos) => {
                client_event.write(ClientEvent::Connected(seed, pos));
            }
            _ => {}
        }
    }
    let mut chunks_to_update = HashSet::new();
    while let Some(message) = client.receive_message(DefaultChannel::ReliableUnordered) {
        let Ok(packet) = bincode::deserialize(&message) else {
            continue;
        };
        if let ServerPacket::ChunkUpdate(chunk_pos, chunk) = packet {
            let mut guard = game_info.chunks.write().unwrap();
            if let Some(old_chunk) = guard.get_mut(&chunk_pos) {
                chunks_to_update.insert(chunk_pos);
                // borrowchecker said no-no to .map()
                let mut saved_chunks = if let Some(saved_chunks) = &game_info.saved_chunks {
                    Some(&mut *saved_chunks.write().unwrap())
                } else {
                    None
                };
                for (pos, block) in chunk.blocks {
                    if pos.x == 0 {
                        chunks_to_update.insert(chunk_pos - IVec3::X);
                    }
                    if pos.x == CHUNK_SIZE - 1 {
                        chunks_to_update.insert(chunk_pos + IVec3::X);
                    }
                    if pos.z == 0 {
                        chunks_to_update.insert(chunk_pos - IVec3::Z);
                    }
                    if pos.z == CHUNK_SIZE - 1 {
                        chunks_to_update.insert(chunk_pos + IVec3::Z);
                    }
                    place_block(old_chunk, pos, block, &mut saved_chunks, None, None);
                }
            }
        }
    }
    if !chunks_to_update.is_empty() {
        update_chunks(
            &mut commands,
            chunks,
            chunks_to_update.into_iter().collect::<Vec<_>>(),
        );
    }

    while let Some(message) = client.receive_message(DefaultChannel::Unreliable) {
        let Ok(packet) = bincode::deserialize(&message) else {
            continue;
        };
        if let ServerPacket::PlayerData(data) = packet {
            for (name, pos) in data {
                if name == game_info.player_name {
                    continue;
                }
                if let Some((_, mut transform, _)) =
                    players.iter_mut().find(|(_, _, player)| player.0 == name)
                {
                    transform.translation = pos + Vec3::Y
                } else {
                    commands
                        .spawn((
                            Mesh3d(meshes.add(Capsule3d::new(0.35, 1.2))),
                            MeshMaterial3d(materials.add(Color::srgb(0.7, 0.7, 0.2))),
                            Transform::from_translation(pos + Vec3::Y),
                            Name::new("Player ".to_string() + &name),
                            OnlinePlayer(name.clone()),
                        ))
                        .with_child((
                            BillboardText::new(name),
                            Transform::from_xyz(0.0, 1.5, 0.0).with_scale(Vec3::splat(0.0125)),
                        ));
                }
            }
        }
    }
}
