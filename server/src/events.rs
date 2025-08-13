use std::collections::{HashMap, VecDeque};

use bevy_math::{Vec3, ivec3};
use ferriscraft::{CHUNK_SIZE, ClientPacket, Persistent, SavedChunk, SavedWorld, ServerPacket};
use renet::{DefaultChannel, RenetServer, ServerEvent};
use renet_netcode::NetcodeServerTransport;

use crate::log;

pub fn handle_events(
    server: &mut RenetServer,
    transport: &mut NetcodeServerTransport,
    logs: &mut VecDeque<String>,
    players: &mut HashMap<u64, (String, Vec3)>,
    persistent_world: &mut Persistent<SavedWorld>,
) {
    let SavedWorld(seed, saved_players, saved_chunks) = &mut persistent_world.data;
    while let Some(event) = server.get_event() {
        match event {
            ServerEvent::ClientConnected { client_id } => {
                log!(
                    logs,
                    "Client {client_id} connecting with ip {}",
                    transport.client_addr(client_id).unwrap()
                );
                let name = String::from_utf8_lossy(&transport.user_data(client_id).unwrap())
                    .trim_end_matches(0 as char)
                    .to_string();
                if players.values().any(|(n, _)| n == &name) {
                    log!(logs, "{name} tried joining but the name is already taken");
                    server.disconnect(client_id);
                    continue;
                }
                log!(logs, "{name} joined the server");
                let pos = saved_players
                    .get(&name)
                    .unwrap_or(&(Vec3::INFINITY, Vec3::ZERO, 0.0, 0.0))
                    .0;
                players.insert(client_id, (name.clone(), pos));
                ServerPacket::PlayerConnected(name, pos).broadcast_except(server, client_id);
                ServerPacket::ConnectionInfo(*seed, pos).send(server, client_id);
                ServerPacket::PlayerData(players.values().cloned().collect()).broadcast(server);
            }
            ServerEvent::ClientDisconnected { client_id, reason } => {
                if let Some((name, pos)) = &players.get(&client_id) {
                    log!(logs, "{name} left the server");
                    ServerPacket::PlayerDisconnected(name.clone(), reason.to_string())
                        .broadcast_except(server, client_id);
                    saved_players.insert(name.clone(), (*pos, Vec3::ZERO, 0.0, 0.0));
                    players.remove(&client_id);
                }
            }
        }
    }

    let client_ids = server.clients_id();
    for &client_id in client_ids.iter() {
        while let Some(message) = server.receive_message(client_id, DefaultChannel::ReliableOrdered)
        {
            let Ok(packet) = bincode::deserialize(&message) else {
                continue;
            };
            match packet {
                ClientPacket::ChatMessage(msg) => {
                    log!(logs, "[{}] {}", players[&client_id].0, msg);
                    ServerPacket::ChatMessage(players[&client_id].0.clone(), msg).broadcast(server);
                }
                ClientPacket::LoadChunks(chunks) => {
                    for chunk in chunks {
                        if let Some(saved_chunk) = saved_chunks.get(&chunk) {
                            ServerPacket::ChunkUpdate(chunk, saved_chunk.clone())
                                .send(server, client_id);
                        }
                    }
                }
                ClientPacket::PlaceBlock(pos, block) => {
                    let chunk_pos = ivec3(
                        pos.x.div_euclid(CHUNK_SIZE),
                        0,
                        pos.z.div_euclid(CHUNK_SIZE),
                    );

                    let block_pos = ivec3(
                        pos.x.rem_euclid(CHUNK_SIZE),
                        pos.y,
                        pos.z.rem_euclid(CHUNK_SIZE),
                    );

                    saved_chunks
                        .entry(chunk_pos)
                        .and_modify(|c| {
                            c.blocks.insert(block_pos, block);
                        })
                        .or_insert(SavedChunk {
                            blocks: HashMap::from([(block_pos, block)]),
                            // entities: Vec::new(),
                        });

                    let player_ids = server
                        .clients_id_iter()
                        .filter(|id| {
                            players
                                .get(id)
                                .unwrap()
                                .1
                                .as_ivec3()
                                .with_y(0)
                                .distance_squared(pos)
                                > 64
                        })
                        .collect::<Vec<_>>();

                    for id in player_ids {
                        if id == client_id {
                            continue;
                        }
                        ServerPacket::ChunkUpdate(
                            chunk_pos,
                            saved_chunks.get(&chunk_pos).unwrap().clone(),
                        )
                        .send(server, id);
                    }
                }
                _ => {}
            }
        }
        while let Some(message) = server.receive_message(client_id, DefaultChannel::Unreliable) {
            let Ok(packet) = bincode::deserialize(&message) else {
                continue;
            };
            match packet {
                ClientPacket::Move(pos) => {
                    players.entry(client_id).and_modify(|x| {
                        x.1 = pos;
                    });
                    ServerPacket::PlayerData(players.values().cloned().collect())
                        .broadcast_except(server, client_id);
                }
                _ => {}
            }
        }
    }
}
