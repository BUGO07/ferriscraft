#![allow(
    clippy::match_like_matches_macro,
    clippy::single_match,
    clippy::too_many_arguments
)]

use std::{
    collections::{HashMap, VecDeque},
    net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket},
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

use bevy_math::Vec3;
use eframe::egui;
use renet::{ConnectionConfig, RenetServer};
use renet_netcode::{NetcodeServerTransport, ServerAuthentication, ServerConfig};

use ferriscraft::{DEFAULT_SERVER_PORT, SavedWorld};

use crate::{events::handle_events, utils::Persistent};

mod events;
mod utils;

struct ServerApp {
    pub private_ip: String,
    pub public_ip: String,
    pub port: String,
    pub max_players: String,
    pub error_message: String,
    pub transport: Option<NetcodeServerTransport>,
    pub server: Option<RenetServer>,
    pub players: HashMap<u64, (String, Vec3)>,
    pub persistent_world: Persistent<SavedWorld>,
    pub last_autosave: Instant,
    pub last_tick: Instant,
    pub accumulator: Duration,
    pub logs: VecDeque<String>,
    pub user_chat_input: String,
}

// TODO: maybe limit fps?
fn main() {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 720.0]),
        ..Default::default()
    };
    eframe::run_native(
        "FerrisCraft Server",
        options,
        Box::new(|_cc| Ok(Box::<ServerApp>::default())),
    )
    .unwrap();
}

impl Default for ServerApp {
    fn default() -> Self {
        Self {
            private_ip: "127.0.0.1".to_string(),
            public_ip: "".to_string(),
            port: DEFAULT_SERVER_PORT.to_string(),
            max_players: 64.to_string(),
            error_message: "".to_string(),
            transport: None,
            server: None,
            players: HashMap::new(),
            persistent_world: Persistent::<SavedWorld>::new(PathBuf::from("saves").join("world.ferris"), SavedWorld(
                    rand::random(),
                    HashMap::new(),
                    HashMap::new(),
                ))
                .expect("World save couldn't be read, please make a backup of saves/world.ferris and remove it from the saves folder."),
            last_autosave: Instant::now(),
            last_tick: Instant::now(),
            accumulator: Duration::ZERO,
            logs: VecDeque::with_capacity(256),
            user_chat_input: "".to_string(),
        }
    }
}

impl ServerApp {
    fn fixed_update(&mut self, dt: Duration) {
        if let Some(ref mut server) = self.server
            && let Some(ref mut transport) = self.transport
        {
            server.update(dt);
            transport.update(dt, server).ok();

            let logs = &mut self.logs;
            let players = &mut self.players;
            let persistent_world = &mut self.persistent_world;

            handle_events(server, transport, logs, players, persistent_world);

            transport.send_packets(server);

            if self.last_autosave.elapsed() > Duration::from_secs(600) {
                save_game(persistent_world, players, logs);
                self.last_autosave = Instant::now();
            }
        }
    }
}

fn stop_server(
    server: &mut Option<RenetServer>,
    transport: &mut Option<NetcodeServerTransport>,
    players: &mut HashMap<u64, (String, Vec3)>,
    persistent_world: &mut Persistent<SavedWorld>,
    logs: &mut VecDeque<String>,
) {
    log!(logs, "Shutting down...");
    if let Some(server) = server
        && let Some(transport) = transport
    {
        transport.disconnect_all(server);
    }
    save_game(persistent_world, players, logs);
    *server = None;
    *transport = None;
    players.clear();
    log!(logs, "Server is offline.");
}

impl eframe::App for ServerApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        stop_server(
            &mut self.server,
            &mut self.transport,
            &mut self.players,
            &mut self.persistent_world,
            &mut self.logs,
        );
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick);
        self.last_tick = now;
        self.accumulator += dt;

        let dt = Duration::from_secs_f64(1.0 / 64.0);

        while self.accumulator >= dt {
            self.fixed_update(dt);
            self.accumulator -= dt;
        }

        let ServerApp {
            private_ip,
            public_ip,
            port,
            max_players,
            error_message,
            transport,
            server,
            players,
            persistent_world,
            last_autosave: _,
            last_tick: _,
            accumulator: _,
            logs,
            user_chat_input,
        } = self;

        ctx.style_mut(|style| {
            style
                .text_styles
                .get_mut(&egui::TextStyle::Body)
                .unwrap()
                .size = 18.0;
            style
                .text_styles
                .get_mut(&egui::TextStyle::Heading)
                .unwrap()
                .size = 26.0;
            style
                .text_styles
                .get_mut(&egui::TextStyle::Button)
                .unwrap()
                .size = 18.0;
        });

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                ui.heading("FerrisCraft Server Panel");
                ui.add_space(10.0);
            });
        });

        egui::SidePanel::new(egui::panel::Side::Left, "left_panel")
            .exact_width(250.0)
            .show(ctx, |ui| {
                if let Some(transport) = transport
                    && let Some(server) = server
                {
                    ui.add_space(10.0);
                    ui.vertical_centered(|ui| {
                        ui.heading(format!(
                            "Players: {} / {}",
                            server.connected_clients(),
                            transport.max_clients()
                        ));
                    });
                    ui.add_space(10.0);

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for client_id in server.clients_id() {
                            let username =
                                String::from_utf8_lossy(&transport.user_data(client_id).unwrap())
                                    .trim_end_matches(0 as char)
                                    .to_string();
                            let rtt = server.rtt(client_id);
                            let addr = transport.client_addr(client_id).unwrap();

                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.label(egui::RichText::new(username).strong());
                                        ui.label(
                                            egui::RichText::new(format!("Ping: {:.2}ms", rtt))
                                                .color(egui::Color32::LIGHT_BLUE),
                                        );
                                        ui.label(
                                            egui::RichText::new(format!("IP: {addr}"))
                                                .color(egui::Color32::GRAY),
                                        );
                                    });

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new("Kick")
                                                        .fill(egui::Color32::from_rgb(200, 50, 50)),
                                                )
                                                .clicked()
                                            {
                                                server.disconnect(client_id);
                                            }
                                        },
                                    );
                                });
                            });
                            ui.add_space(6.0);
                        }
                    });
                } else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(10.0);
                        ui.heading("Server Settings");
                        ui.add_space(10.0);

                        ui.label("Private IP:");
                        ui.add_sized(
                            [200.0, 28.0],
                            egui::TextEdit::singleline(private_ip)
                                .horizontal_align(egui::Align::Center),
                        );

                        ui.label("Public IP:")
                            .on_hover_text_at_pointer("Leave empty for LAN-only.");
                        ui.add_sized(
                            [200.0, 28.0],
                            egui::TextEdit::singleline(public_ip)
                                .horizontal_align(egui::Align::Center),
                        )
                        .on_hover_text_at_pointer("Leave empty for LAN-only.");

                        ui.label("Port:");
                        ui.add_sized(
                            [200.0, 28.0],
                            egui::TextEdit::singleline(port).horizontal_align(egui::Align::Center),
                        );

                        ui.label("Max Players:");
                        ui.add_sized(
                            [200.0, 28.0],
                            egui::TextEdit::singleline(max_players)
                                .horizontal_align(egui::Align::Center),
                        );
                    });
                }
            });
        egui::SidePanel::new(egui::panel::Side::Right, "right_panel")
            .exact_width(250.0)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(10.0);
                    ui.heading("Resources"); // TODO maybe cpu and ram usage and stuff
                    ui.add_space(10.0);
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                if !error_message.is_empty() {
                    ui.colored_label(egui::Color32::RED, error_message.clone());
                    ui.add_space(5.0);
                    ui.separator();
                }

                ui.add_space(10.0);

                if transport.is_some() && server.is_some() {
                    ui.colored_label(egui::Color32::LIGHT_GREEN, "Server is running");
                    ui.add_space(10.0);

                    if ui
                        .add_sized([240.0, 40.0], egui::Button::new("Stop Server"))
                        .clicked()
                    {
                        stop_server(server, transport, players, persistent_world, logs);
                    }
                } else {
                    ui.colored_label(egui::Color32::LIGHT_RED, "Server is offline");
                    ui.add_space(10.0);
                    if ui
                        .add_sized([240.0, 40.0], egui::Button::new("Start Server"))
                        .clicked()
                    {
                        ui.scope(|_| {
                            let Ok(port) = port.parse::<u16>() else {
                                *error_message = "Invalid port".to_string();
                                return;
                            };
                            let Ok(private_ip) = private_ip.parse::<Ipv4Addr>() else {
                                *error_message = "Invalid private IP".to_string();
                                return;
                            };
                            let ips = vec![SocketAddr::V4(SocketAddrV4::new(
                                if !public_ip.is_empty() {
                                    if let Ok(public_ip) = public_ip.parse::<Ipv4Addr>() {
                                        public_ip
                                    } else {
                                        *error_message = "Invalid public IP".to_string();
                                        return;
                                    }
                                } else {
                                    private_ip
                                },
                                port,
                            ))];
                            let Ok(max_clients) = max_players.parse::<usize>() else {
                                *error_message = "Invalid max players".to_string();
                                return;
                            };
                            if max_clients > 1024 {
                                *error_message = "Max players too high".to_string();
                                return;
                            }

                            *error_message = "".to_string();

                            let version = env!("CARGO_PKG_VERSION");

                            log!(logs, "Starting server...");
                            log!(logs, "Server Version - {version}");
                            log!(logs, "Binding to {}", ips[0]);
                            let socket = match UdpSocket::bind(ips[0]) {
                                Ok(socket) => socket,
                                Err(error) => {
                                    log!(logs, "Failed to bind to {}: {error}", ips[0]);
                                    return;
                                }
                            };

                            let split = version.split(".").collect::<Vec<_>>();
                            let protocol_id = split[0].parse::<u64>().unwrap() * 1_000_000
                                + split[1].parse::<u64>().unwrap() * 1_000
                                + split[2].parse::<u64>().unwrap();

                            log!(logs, "Protocol ID - {protocol_id}");

                            let server_config = ServerConfig {
                                current_time: SystemTime::now()
                                    .duration_since(SystemTime::UNIX_EPOCH)
                                    .expect("system clock is wrong"),
                                max_clients,
                                protocol_id,
                                public_addresses: ips,
                                authentication: ServerAuthentication::Unsecure,
                            };

                            log!(logs, "Initializing server...");
                            *server = Some(RenetServer::new(ConnectionConfig::default()));
                            log!(logs, "Initializing transport layer...");
                            *transport =
                                Some(NetcodeServerTransport::new(server_config, socket).unwrap());
                            log!(logs, "Up and running!");
                        });
                    }
                }

                ui.add_space(10.0);

                ui.vertical(|ui| {
                    ui.style_mut()
                        .text_styles
                        .get_mut(&egui::TextStyle::Body)
                        .unwrap()
                        .size = 16.0;
                    egui::Frame::canvas(ui.style()).show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(ui.available_height() - 40.0)
                            .stick_to_bottom(true)
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                for message in logs.iter() {
                                    ui.label(message);
                                }
                            });
                    });
                });
                ui.add_space(4.0);

                let input_response = ui.add_sized(
                    [ui.available_width(), 28.0],
                    egui::TextEdit::singleline(user_chat_input).desired_width(f32::INFINITY),
                );

                if input_response.lost_focus()
                    && ui.input(|state| state.key_pressed(egui::Key::Enter))
                {
                    input_response.request_focus();
                    if server.is_some() && transport.is_some() {
                        let message = user_chat_input.trim();
                        if !message.is_empty() {
                            if !message.starts_with("/") {
                                log!(logs, "[Server] {}", message);
                            } else {
                                match message {
                                    "/save" => {
                                        save_game(persistent_world, players, logs);
                                    }
                                    "/stop" => {
                                        stop_server(
                                            server,
                                            transport,
                                            players,
                                            persistent_world,
                                            logs,
                                        );
                                    }
                                    _ => {
                                        log!(logs, "Unknown command: {}", message);
                                    }
                                }
                            }
                            user_chat_input.clear();
                        }
                    }
                }
            });
        });
        ctx.request_repaint();
    }
}

pub fn save_game(
    persistent_world: &mut Persistent<SavedWorld>,
    players: &HashMap<u64, (String, Vec3)>,
    logs: &mut VecDeque<String>,
) {
    log!(logs, "Saving...");
    // chunks are updated in Persistent<SavedWorld>
    if let Err(error) = persistent_world.update(|SavedWorld(_, saved_players, _)| {
        for (_player_id, (name, pos)) in players.iter() {
            saved_players.insert(name.clone(), (*pos, Vec3::ZERO, 0.0, 0.0));
        }
    }) {
        log!(logs, "Failed to save game - {error}");
    }
}
