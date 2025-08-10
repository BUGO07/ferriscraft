#![allow(clippy::match_like_matches_macro, clippy::single_match)]

use std::{
    collections::HashMap,
    net::{SocketAddr, SocketAddrV4, UdpSocket},
    path::Path,
    time::SystemTime,
};

use bevy::{log::LogPlugin, prelude::*};
use bevy_persistent::{Persistent, StorageFormat};
use bevy_renet::{
    RenetServerPlugin,
    netcode::{NetcodeServerPlugin, NetcodeServerTransport, ServerAuthentication, ServerConfig},
    renet::{ConnectionConfig, DefaultChannel, RenetServer, ServerEvent},
};
use ferriscraft::{
    ClientPacket, DEFAULT_SERVER_PORT, PlayerData, SavedChunk, SavedWorld, ServerPacket,
};

fn main() {
    let mut app = App::new();

    app.add_plugins((MinimalPlugins, LogPlugin::default(), RenetServerPlugin, NetcodeServerPlugin))
        .insert_resource(RenetServer::new(ConnectionConfig::default())).insert_resource(
            Persistent::<SavedWorld>::builder()
                .name("saved world")
                .format(StorageFormat::Bincode)
                .path(Path::new("saves").join("world.ferris"))
                .default(SavedWorld(
                    rand::random(),
                    HashMap::new(),
                    HashMap::new(),
                ))
                .build()
                .expect("World save couldn't be read, please make a backup of saves/world.ferris and remove it from the saves folder."),
        )
        .init_resource::<PlayerData>();

    let server_addr = SocketAddr::V4(SocketAddrV4::new(
        "127.0.0.1".parse().unwrap(),
        DEFAULT_SERVER_PORT,
    ));
    let socket = UdpSocket::bind(server_addr).unwrap();
    let server_config = ServerConfig {
        current_time: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap(),
        max_clients: 64,
        protocol_id: 0,
        public_addresses: vec![server_addr],
        authentication: ServerAuthentication::Unsecure,
    };

    app.insert_resource(NetcodeServerTransport::new(server_config, socket).unwrap());

    app.add_systems(FixedUpdate, handle_events_system);

    app.run();
}

#[derive(Resource)]
pub struct GameInfo {
    pub saved_chunks: HashMap<IVec3, SavedChunk>,
}

pub fn save_game(
    persistent_world: &mut ResMut<Persistent<SavedWorld>>,
    // player: &Transform,
    // camera: &Transform,
    // velocity: Vec3,
    game_info: &GameInfo,
) {
    persistent_world
        .update(|sc| {
            // sc.1.0 = player.translation;
            // sc.1.1 = velocity;
            // let (_, pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
            // let (yaw, _, _) = player.rotation.to_euler(EulerRot::YXZ);
            // sc.1.2 = yaw;
            // sc.1.3 = pitch;
            sc.2 = game_info.saved_chunks.clone();
        })
        .unwrap();
}

fn handle_events_system(
    mut server: ResMut<RenetServer>,
    transport: Res<NetcodeServerTransport>,
    mut server_events: EventReader<ServerEvent>,
    mut player_data: ResMut<PlayerData>,
    mut persistent_world: ResMut<Persistent<SavedWorld>>,
) {
    'event: for event in server_events.read() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                println!("Client {client_id} connected");
                let name = String::from_utf8_lossy(&transport.user_data(*client_id).unwrap())
                    .trim_end_matches(0 as char)
                    .to_string();
                for (n, _) in player_data.0.values() {
                    if n == &name {
                        println!("Player {name} already connected");
                        server.disconnect(*client_id);
                        continue 'event;
                    }
                }
                let pos = persistent_world
                    .1
                    .get(&name)
                    .unwrap_or(&(Vec3::INFINITY, Vec3::ZERO, 0.0, 0.0))
                    .0;
                player_data.0.insert(*client_id, (name, pos));
                server.broadcast_message(
                    DefaultChannel::ReliableOrdered,
                    bincode::serialize(&ServerPacket::ClientConnected(*client_id, pos)).unwrap(),
                );
                server.send_message(
                    *client_id,
                    DefaultChannel::ReliableOrdered,
                    bincode::serialize(&ServerPacket::ConnectionInfo(persistent_world.0, pos))
                        .unwrap(),
                );
                server.send_message(
                    *client_id,
                    DefaultChannel::Unreliable,
                    bincode::serialize(&ServerPacket::PlayerData(player_data.0.clone())).unwrap(),
                );
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                println!("Client {client_id} disconnected: {reason}");
                server.broadcast_message_except(
                    *client_id,
                    DefaultChannel::ReliableOrdered,
                    bincode::serialize(&ServerPacket::ClientDisconnected(
                        *client_id,
                        reason.to_string(),
                    ))
                    .unwrap(),
                );
                persistent_world
                    .update(|sc| {
                        let default = ("".into(), Vec3::INFINITY);
                        let player_data = player_data.0.get(client_id).unwrap_or(&default);
                        sc.1.insert(player_data.0.clone(), (player_data.1, Vec3::ZERO, 0.0, 0.0));
                    })
                    .unwrap();
                player_data.0.remove(client_id);
            }
        }
    }

    let client_ids = server.clients_id();
    for &client_id in client_ids.iter() {
        while let Some(message) = server.receive_message(client_id, DefaultChannel::ReliableOrdered)
        {
            let packet: ClientPacket = bincode::deserialize(&message).unwrap();
            println!("Client {client_id} sent a packet: {packet:?}");
            match packet {
                ClientPacket::ChatMessage(msg) => {
                    server.broadcast_message_except(
                        client_id,
                        DefaultChannel::ReliableOrdered,
                        bincode::serialize(&ServerPacket::ChatMessage(msg)).unwrap(),
                    );
                }
                ClientPacket::PlaceBlock(_pos, _block) => {
                    // TODO
                }
                _ => {}
            }
        }
        while let Some(message) = server.receive_message(client_id, DefaultChannel::Unreliable) {
            let packet: ClientPacket = bincode::deserialize(&message).unwrap();
            match packet {
                ClientPacket::Move(pos) => {
                    player_data.0.entry(client_id).and_modify(|x| {
                        x.1 = pos;
                    });
                    server.broadcast_message_except(
                        client_id,
                        DefaultChannel::Unreliable,
                        bincode::serialize(&ServerPacket::PlayerData(player_data.0.clone()))
                            .unwrap(),
                    );
                    println!("{:?}", player_data.0);
                }
                _ => {}
            }
        }
    }
}
