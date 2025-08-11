#![allow(clippy::match_like_matches_macro, clippy::single_match)]

use std::{
    collections::{HashMap, hash_map::Entry},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    path::Path,
    time::SystemTime,
};

use bevy::{prelude::*, window::PrimaryWindow};
use bevy_inspector_egui::{
    bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass},
    egui,
};
use bevy_persistent::{Persistent, StorageFormat};
use bevy_renet::{
    RenetServerPlugin,
    netcode::{NetcodeServerPlugin, NetcodeServerTransport, ServerAuthentication, ServerConfig},
    renet::{ConnectionConfig, DefaultChannel, RenetServer, ServerEvent},
};
use ferriscraft::{
    CHUNK_SIZE, ClientPacket, DEFAULT_SERVER_PORT, PlayerData, SavedChunk, SavedWorld,
    ServerPacket, hash,
};

fn main() {
    let mut app = App::new();

    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "FerrisCraft Server".to_string(),
                ..default()
            }),
            ..default()
        }),
        EguiPlugin::default(),
        RenetServerPlugin,
        NetcodeServerPlugin
        ))
    .insert_resource(ServerSettings {
        private_ip: "127.0.0.1".to_string(),
        public_ip: "127.0.0.1".to_string(),
        port: DEFAULT_SERVER_PORT.to_string(),
        max_players: 64.to_string(),
        error_message: "".to_string(),
        running: false,
    })
    .insert_resource(
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

    // let private_ip = SocketAddr::V4(SocketAddrV4::new(
    //     "127.0.0.1".parse().unwrap(),
    //     DEFAULT_SERVER_PORT,
    // ));

    // let public_ip = SocketAddr::V4(SocketAddrV4::new(
    //     "127.0.0.1".parse().unwrap(),
    //     DEFAULT_SERVER_PORT,
    // ));

    // let socket = UdpSocket::bind(private_ip).unwrap();
    // let server_config = ServerConfig {
    //     current_time: SystemTime::now()
    //         .duration_since(SystemTime::UNIX_EPOCH)
    //         .unwrap(),
    //     max_clients: 64,
    //     protocol_id: 0,
    //     public_addresses: vec![public_ip],
    //     authentication: ServerAuthentication::Unsecure,
    // };

    // app.insert_resource(RenetServer::new(ConnectionConfig::default()));
    // app.insert_resource(NetcodeServerTransport::new(server_config, socket).unwrap());

    app.add_systems(Startup, setup)
        .add_systems(EguiPrimaryContextPass, handle_ui)
        .add_systems(
            FixedUpdate,
            handle_events.run_if(|server_settings: Res<ServerSettings>| server_settings.running),
        );

    app.run();
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
}

#[derive(Resource)]
pub struct GameInfo {
    pub saved_chunks: HashMap<IVec3, SavedChunk>,
}

#[derive(Resource)]
pub struct ServerSettings {
    pub private_ip: String,
    pub public_ip: String,
    pub port: String,
    pub max_players: String,
    pub error_message: String,
    pub running: bool,
}

fn handle_ui(
    mut commands: Commands,
    mut contexts: EguiContexts,
    mut server_settings: ResMut<ServerSettings>,
    transport: Option<ResMut<NetcodeServerTransport>>,
    server: Option<ResMut<RenetServer>>,
) -> Result {
    egui::CentralPanel::default().show(contexts.ctx_mut()?, |ui| {
        if server_settings.running
            && let Some(mut transport) = transport
            && let Some(mut server) = server
        {
            ui.label("server running");
            if ui.button("stop server").clicked() {
                transport.disconnect_all(&mut server);
                commands.remove_resource::<RenetServer>();
                commands.remove_resource::<NetcodeServerTransport>();
                server_settings.running = false;
            }
        } else {
            ui.label(server_settings.error_message.clone());
            ui.label("private ip");
            ui.text_edit_singleline(&mut server_settings.private_ip);
            ui.label("public ip (leave empty if you don't plan on port forwarding)");
            ui.text_edit_singleline(&mut server_settings.public_ip);
            ui.label("port");
            ui.text_edit_singleline(&mut server_settings.port);
            ui.label("max players");
            ui.text_edit_singleline(&mut server_settings.max_players);

            if ui.button("start server").clicked() {
                let Ok(port) = server_settings.port.parse::<u16>() else {
                    server_settings.error_message = "invalid port".to_string();
                    return;
                };
                let Ok(private_ip) = server_settings.private_ip.parse::<Ipv4Addr>() else {
                    server_settings.error_message = "invalid private ip".to_string();
                    return;
                };
                let public_ip = if server_settings.public_ip.is_empty() {
                    if let Ok(public_ip) = server_settings.public_ip.parse::<Ipv4Addr>() {
                        public_ip
                    } else {
                        server_settings.error_message = "invalid public ip".to_string();
                        return;
                    }
                } else {
                    private_ip
                };
                let Ok(max_clients) = server_settings.max_players.parse::<usize>() else {
                    server_settings.error_message =
                        "invalid max players (must be a number)".to_string();
                    return;
                };
                if max_clients > 1024 {
                    server_settings.error_message = "max players too high".to_string();
                    return;
                }

                let mut ips = vec![SocketAddr::V4(SocketAddrV4::new(private_ip, port))];
                if private_ip != public_ip {
                    ips.push(SocketAddr::V4(SocketAddrV4::new(public_ip, port)));
                }

                let socket = UdpSocket::bind(ips[0]).unwrap();

                let server_config = ServerConfig {
                    current_time: SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap(),
                    max_clients,
                    protocol_id: hash(env!("CARGO_PKG_VERSION")),
                    public_addresses: ips,
                    authentication: ServerAuthentication::Unsecure,
                };

                commands.insert_resource(RenetServer::new(ConnectionConfig::default()));
                commands
                    .insert_resource(NetcodeServerTransport::new(server_config, socket).unwrap());

                server_settings.running = true;
            }
        }
    });
    Ok(())
}
pub fn autosave(
    mut app_exit: EventWriter<AppExit>,
    mut last_save: Local<f32>,
    persistent_world: Option<ResMut<Persistent<SavedWorld>>>,
    server: Option<ResMut<RenetServer>>,
    window: Query<&Window, With<PrimaryWindow>>,
    game_info: Res<GameInfo>,
    time: Res<Time>,
) {
    if window.is_empty() {
        info!("saving and exiting");
        // if let Ok(player) = player.single()
        //     && let Ok(camera) = camera.single()
        // {
        save_game(
            persistent_world,
            // player.0,
            // camera,
            // player.1.velocity,
            &game_info,
        );
        // }
        if let Some(mut client) = server {
            client.disconnect_all();
        }
        app_exit.write(AppExit::Success);
        return;
    }

    let elapsed = time.elapsed_secs_wrapped();

    if elapsed > *last_save + 60.0 {
        // if let Ok(player) = player.single()
        //     && let Ok(camera) = camera.single()
        // {
        save_game(
            persistent_world,
            // player.0,
            // camera,
            // player.1.velocity,
            &game_info,
        );
        // }
        *last_save = elapsed;
    }

    if elapsed < *last_save {
        *last_save = elapsed;
    }
}

pub fn save_game(
    persistent_world: Option<ResMut<Persistent<SavedWorld>>>,
    // player: &Transform,
    // camera: &Transform,
    // velocity: Vec3,
    game_info: &GameInfo,
) {
    if let Some(mut persistent_world) = persistent_world {
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
}

fn handle_events(
    mut server: ResMut<RenetServer>,
    transport: Res<NetcodeServerTransport>,
    mut server_events: EventReader<ServerEvent>,
    mut player_data: ResMut<PlayerData>,
    mut persistent_world: ResMut<Persistent<SavedWorld>>,
    mut game_info: ResMut<GameInfo>,
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
                ClientPacket::PlaceBlock(pos, block) => {
                    match game_info.saved_chunks.entry(ivec3(
                        pos.x.div_euclid(CHUNK_SIZE),
                        0,
                        pos.z.div_euclid(CHUNK_SIZE),
                    )) {
                        Entry::Vacant(e) => {
                            e.insert(SavedChunk {
                                blocks: HashMap::from([(
                                    ivec3(
                                        pos.x.rem_euclid(CHUNK_SIZE),
                                        pos.y,
                                        pos.z.rem_euclid(CHUNK_SIZE),
                                    ),
                                    block,
                                )]),
                                entities: Vec::new(),
                            });
                        }
                        Entry::Occupied(mut e) => {
                            e.get_mut().blocks.insert(pos, block);
                        }
                    }
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
