#![allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::match_like_matches_macro,
    clippy::vec_init_then_push
)]

use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hasher},
};

use bevy::prelude::*;
use bevy_renet::renet::{DefaultChannel, RenetClient, RenetServer};
use serde::{Deserialize, Serialize};

pub const DEFAULT_SERVER_PORT: u16 = 42069;

pub const CHUNK_SIZE: i32 = 16; // MAX 63
pub const CHUNK_HEIGHT: i32 = 256; // MAX 511
pub const SEA_LEVEL: i32 = 64; // MAX CHUNK_HEIGHT - 180

#[derive(Serialize, Deserialize, Debug)]
pub enum ClientPacket {
    ChatMessage(String),
    PlaceBlock(IVec3, Block),
    LoadChunks(Vec<IVec3>),
    Move(Vec3),
}

impl ClientPacket {
    fn channel(&self) -> DefaultChannel {
        match self {
            ClientPacket::ChatMessage(_) => DefaultChannel::ReliableOrdered,
            ClientPacket::PlaceBlock(_, _) => DefaultChannel::ReliableOrdered,
            ClientPacket::LoadChunks(_) => DefaultChannel::ReliableOrdered,
            ClientPacket::Move(_) => DefaultChannel::Unreliable,
        }
    }
    pub fn send(&mut self, client: Option<ResMut<RenetClient>>) {
        if let Some(mut client) = client {
            client.send_message(self.channel(), bincode::serialize(self).unwrap());
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ServerPacket {
    ChatMessage(String, String),        // player, message
    PlayerConnected(String, Vec3),      // player, pos
    PlayerDisconnected(String, String), // player, reason
    ConnectionInfo(u32, Vec3),          // seed, pos
    ChunkUpdate(IVec3, SavedChunk),     // pos, chunk
    PlayerData(HashMap<String, Vec3>),  // player, pos
}

impl ServerPacket {
    fn channel(&self) -> DefaultChannel {
        match self {
            ServerPacket::ChatMessage(_, _) => DefaultChannel::ReliableOrdered,
            ServerPacket::PlayerConnected(_, _) => DefaultChannel::ReliableOrdered,
            ServerPacket::PlayerDisconnected(_, _) => DefaultChannel::ReliableOrdered,
            ServerPacket::ConnectionInfo(_, _) => DefaultChannel::ReliableOrdered,
            ServerPacket::ChunkUpdate(_, _) => DefaultChannel::ReliableUnordered,
            ServerPacket::PlayerData(_) => DefaultChannel::Unreliable,
        }
    }
    pub fn broadcast(&mut self, server: &mut RenetServer) {
        server.broadcast_message(self.channel(), bincode::serialize(self).unwrap());
    }
    pub fn broadcast_except(&mut self, server: &mut RenetServer, client_id: u64) {
        server.broadcast_message_except(
            client_id,
            self.channel(),
            bincode::serialize(self).unwrap(),
        );
    }
    pub fn send(&mut self, server: &mut RenetServer, client_id: u64) {
        server.send_message(client_id, self.channel(), bincode::serialize(self).unwrap());
    }
}

#[inline]
pub fn hash(value: impl std::hash::Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct SavedChunk {
    pub entities: Vec<(Entity, GameEntity)>,
    pub blocks: HashMap<IVec3, Block>, // placed/broken blocks
}

#[derive(Resource, Clone, Debug, Default, Serialize, Deserialize)]
pub struct SavedWorld(
    pub u32,
    // name, (transform, velocity, yaw, pitch)
    pub HashMap<String, (Vec3, Vec3, f32, f32)>,
    pub HashMap<IVec3, SavedChunk>,
);

#[derive(Component, Clone, Copy, Serialize, Deserialize, PartialEq, Debug)]
pub struct GameEntity {
    pub kind: GameEntityKind,
    pub pos: Vec3,
    pub rot: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Serialize, Deserialize)]
pub enum BlockKind {
    #[default]
    Air,
    Stone,
    Dirt,
    Grass,
    Plank,
    Bedrock,
    Water,
    Sand,
    Wood,
    Leaf,
    Snow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GameEntityKind {
    Ferris,
}

#[derive(Component, Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub kind: BlockKind,
    pub direction: Direction,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Serialize, Deserialize)]
pub enum Direction {
    Left,
    Right,
    Bottom,
    #[default]
    Top,
    Back,
    Front,
}

#[allow(dead_code)]
impl Block {
    pub const DEFAULT: Self = Self::AIR;
    pub const AIR: Self = Self {
        kind: BlockKind::Air,
        direction: Direction::Top,
    };
    pub const STONE: Self = Self {
        kind: BlockKind::Stone,
        ..Self::DEFAULT
    };
    pub const DIRT: Self = Self {
        kind: BlockKind::Dirt,
        ..Self::DEFAULT
    };
    pub const GRASS: Self = Self {
        kind: BlockKind::Grass,
        ..Self::DEFAULT
    };
    pub const PLANK: Self = Self {
        kind: BlockKind::Plank,
        ..Self::DEFAULT
    };
    pub const BEDROCK: Self = Self {
        kind: BlockKind::Bedrock,
        ..Self::DEFAULT
    };
    pub const WATER: Self = Self {
        kind: BlockKind::Water,
        ..Self::DEFAULT
    };
    pub const SAND: Self = Self {
        kind: BlockKind::Sand,
        ..Self::DEFAULT
    };
    pub const WOOD: Self = Self {
        kind: BlockKind::Wood,
        ..Self::DEFAULT
    };
    pub const LEAF: Self = Self {
        kind: BlockKind::Leaf,
        ..Self::DEFAULT
    };
    pub const SNOW: Self = Self {
        kind: BlockKind::Snow,
        ..Self::DEFAULT
    };
}

impl BlockKind {
    #[inline]
    pub fn is_solid(self) -> bool {
        self != BlockKind::Air && self != BlockKind::Water
    }
    #[inline]
    pub fn is_air(self) -> bool {
        self == BlockKind::Air
    }
    #[inline]
    pub fn can_rotate(self) -> bool {
        match self {
            BlockKind::Wood => true,
            _ => false,
        }
    }
    #[inline]
    pub fn from_u32(value: u32) -> BlockKind {
        match value {
            0 => BlockKind::Air,
            1 => BlockKind::Stone,
            2 => BlockKind::Dirt,
            3 => BlockKind::Grass,
            4 => BlockKind::Plank,
            5 => BlockKind::Bedrock,
            6 => BlockKind::Water,
            7 => BlockKind::Sand,
            8 => BlockKind::Wood,
            9 => BlockKind::Leaf,
            10 => BlockKind::Snow,
            _ => BlockKind::Air,
        }
    }
}

impl Direction {
    pub const NORMALS: &[Vec3; 6] = &[
        vec3(-1.0, 0.0, 0.0), // Left
        vec3(1.0, 0.0, 0.0),  // Right
        vec3(0.0, -1.0, 0.0), // Bottom
        vec3(0.0, 1.0, 0.0),  // Top
        vec3(0.0, 0.0, -1.0), // Back
        vec3(0.0, 0.0, 1.0),  // Front
    ];

    #[inline]
    pub fn as_vec3(self) -> Vec3 {
        Self::NORMALS[self as usize]
    }

    #[inline]
    pub fn get_opposite(self) -> Self {
        match self {
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
            Direction::Bottom => Direction::Top,
            Direction::Top => Direction::Bottom,
            Direction::Back => Direction::Front,
            Direction::Front => Direction::Back,
        }
    }

    #[inline]
    pub fn get_uvs(self, block: Block) -> [Vec2; 4] {
        const ATLAS_SIZE_X: f32 = 3.0;
        const ATLAS_SIZE_Y: f32 = 10.0;

        let face_idx = match self {
            d if d == block.direction => 0.0,
            d if d == block.direction.get_opposite() => 2.0,
            _ => 1.0,
        };

        let pos = vec2(
            face_idx / ATLAS_SIZE_X,
            (block.kind as u32 - 1) as f32 / ATLAS_SIZE_Y,
        );

        let base = [
            vec2(pos.x, pos.y + 1.0 / ATLAS_SIZE_Y),
            vec2(pos.x, pos.y),
            vec2(pos.x + 1.0 / ATLAS_SIZE_X, pos.y),
            vec2(pos.x + 1.0 / ATLAS_SIZE_X, pos.y + 1.0 / ATLAS_SIZE_Y),
        ];
        let rotate_90 = [base[3], base[0], base[1], base[2]];
        let rotate_180 = [base[2], base[3], base[0], base[1]];
        let rotate_270 = [base[1], base[2], base[3], base[0]];

        // HOLY BAD CODE
        use Direction::*;
        match (block.direction, self) {
            (Right, Top | Bottom) => base,
            (Right, Back) => rotate_90,
            (Right, _) => rotate_270,
            (Top, Front | Back) => base,
            (Top, Left) => rotate_90,
            (Top, _) => rotate_270,
            (Front, Right | Left) => base,
            (Front, Bottom) => rotate_90,
            (Front, _) => rotate_270,
            (Left, Top | Bottom) => rotate_180,
            (Left, Back) => rotate_270,
            (Left, _) => rotate_90,
            (Bottom, Front | Back) => rotate_180,
            (Bottom, Left) => rotate_270,
            (Bottom, _) => rotate_90,
            (Back, Right | Left) => rotate_180,
            (Back, Bottom) => rotate_270,
            (Back, _) => rotate_90,
        }
    }
}
