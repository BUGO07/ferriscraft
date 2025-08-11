use std::{
    net::{SocketAddr, UdpSocket},
    time::SystemTime,
};

use bevy::{prelude::*, window::PrimaryWindow};
use bevy_renet::{
    RenetClientPlugin,
    netcode::{
        ClientAuthentication, NETCODE_USER_DATA_BYTES, NetcodeClientPlugin, NetcodeClientTransport,
    },
    renet::{ConnectionConfig, DefaultChannel, RenetClient},
};
use ferriscraft::{BlockKind, ClientPacket, ServerPacket};
use iyes_perf_ui::prelude::PerfUiAllEntries;

use crate::{
    GameInfo,
    player::{OnlinePlayer, camera_bundle, player_bundle},
    ui::{GameState, coords_bundle, hotbar_block, hotbar_bundle, root_ui_bundle},
    utils::{get_noise_functions, toggle_grab_cursor},
};

pub struct MultiplayerPlugin;

impl Plugin for MultiplayerPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.add_plugins((RenetClientPlugin, NetcodeClientPlugin))
            .add_event::<ServerConnect>()
            .add_systems(OnEnter(GameState::MultiPlayer), setup)
            .add_systems(
                Update,
                (on_connect, send_client_data, receive_server_data)
                    .run_if(in_state(GameState::MultiPlayer)),
            );
    }
}

#[derive(Resource)]
pub struct MultiplayerMenuInput(pub SocketAddr, pub String);

fn setup(mut commands: Commands, multiplayer_input: Res<MultiplayerMenuInput>) {
    let client_id = rand::random();

    let mut user_data = [0; NETCODE_USER_DATA_BYTES];
    // let name = std::env::args()
    //     .nth(1)
    //     .unwrap_or(format!("Player {}", rand::random_range(0..1000)));
    let bytes = multiplayer_input.1.as_bytes();
    user_data[..bytes.len()].copy_from_slice(bytes);
    commands.insert_resource(RenetClient::new(ConnectionConfig::default()));
    commands.insert_resource(
        NetcodeClientTransport::new(
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap(),
            ClientAuthentication::Unsecure {
                server_addr: multiplayer_input.0,
                client_id,
                user_data: Some(user_data),
                protocol_id: 0,
            },
            UdpSocket::bind("0.0.0.0:0").unwrap(),
        )
        .unwrap(),
    );
}

#[derive(Event)]
pub struct ServerConnect(pub u32, pub Vec3);

fn on_connect(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut connect_event: EventReader<ServerConnect>,
    // persistent_world: Res<Persistent<SavedWorld>>,
    camera: Single<(Entity, &mut Camera3d)>,
    asset_server: Res<AssetServer>,
) {
    for &ServerConnect(seed, pos) in connect_event.read() {
        info!("Connected to server");
        let mut mats = Vec::new();
        mats.push(materials.add(StandardMaterial {
            base_color_texture: Some(asset_server.load("atlas.ktx2")),
            reflectance: 0.0,
            ..default()
        }));
        let mut models = Vec::new();
        models.push(asset_server.load(GltfAssetLabel::Scene(0).from_asset("models/ferris.glb")));

        let game_info = GameInfo {
            noises: get_noise_functions(seed),
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

        commands.insert_resource(game_info);
    }
    // let &SavedWorld(seed, _, ref saved_chunks) = persistent_world.get();
}

fn send_client_data(
    mut app_exit: EventWriter<AppExit>,
    client: ResMut<RenetClient>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if client.is_disconnected() {
        app_exit.write(AppExit::Success);
    }
    if keyboard.just_pressed(KeyCode::KeyT) {
        ClientPacket::ChatMessage("shice".into()).send(Some(client));
    }
}

fn receive_server_data(
    mut commands: Commands,
    mut client: ResMut<RenetClient>,
    mut players: Query<(Entity, &mut Transform, &OnlinePlayer)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut connect_event: EventWriter<ServerConnect>,
    transport: Res<NetcodeClientTransport>,
) {
    while let Some(message) = client.receive_message(DefaultChannel::ReliableOrdered) {
        let packet: ServerPacket = bincode::deserialize(&message).unwrap();
        match packet {
            ServerPacket::ChatMessage(msg) => {
                println!("Received chat message: {msg}");
            }
            ServerPacket::ClientConnected(id, pos) => {
                if id != transport.client_id() {
                    commands.spawn((
                        Mesh3d(meshes.add(Cuboid::new(1.0, 2.0, 1.0))),
                        MeshMaterial3d(materials.add(StandardMaterial::default())),
                        Transform::from_translation(pos),
                        Name::new("Player".to_string() + &id.to_string()),
                        OnlinePlayer(id),
                    ));
                }
            }
            ServerPacket::ClientDisconnected(id, reason) => {
                if id != transport.client_id()
                    && let Some((entity, _, _)) =
                        players.iter_mut().find(|(_, _, other)| other.0 == id)
                {
                    commands.entity(entity).despawn();
                }
                println!("Client {id} disconnected: {reason}");
            }
            ServerPacket::ConnectionInfo(seed, pos) => {
                connect_event.write(ServerConnect(seed, pos));
            }
            _ => {}
        }
    }
    while let Some(message) = client.receive_message(DefaultChannel::Unreliable) {
        let packet: ServerPacket = bincode::deserialize(&message).unwrap();
        if let ServerPacket::PlayerData(data) = packet {
            for (&id, (name, pos)) in data.iter() {
                if id == transport.client_id() {
                    continue;
                }
                if let Some((_, mut transform, _)) =
                    players.iter_mut().find(|(_, _, player)| player.0 == id)
                {
                    *transform = Transform::from_translation(pos + Vec3::Y);
                } else {
                    commands.spawn((
                        Mesh3d(meshes.add(Cuboid::new(1.0, 2.0, 1.0))),
                        MeshMaterial3d(materials.add(StandardMaterial::default())),
                        Transform::from_translation(pos + Vec3::Y),
                        Name::new("Player".to_string() + name),
                        OnlinePlayer(id),
                    ));
                }
            }
        }
    }
}
