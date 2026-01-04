#![allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::match_like_matches_macro,
    clippy::vec_init_then_push
)]

use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hasher},
    ops::{Deref, DerefMut},
    path::PathBuf,
};

use bevy_math::{IVec3, Vec2, Vec3, vec2, vec3};
use renet::{DefaultChannel, RenetServer};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[cfg(feature = "client")]
use bevy_ecs::prelude::*;
#[cfg(feature = "client")]
use renet::RenetClient;

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

#[cfg(feature = "client")]
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

#[cfg_attr(feature = "client", derive(Resource))]
pub struct Persistent<R: Serialize + DeserializeOwned> {
    pub path: PathBuf,
    pub data: R,
    human: bool,
}

impl<R: Serialize + DeserializeOwned> Persistent<R> {
    pub fn new(path: PathBuf, default: R, human: bool) -> Self {
        let mut persistent = Self {
            path: path.clone(),
            data: default,
            human,
        };

        if !path.exists() {
            let init = persistent.initialize();
            let write = persistent.write();
            if init.is_ok() && write.is_ok() {
                return persistent;
            } else {
                println!(
                    "Failed to initialize: {}, data won't be saved. error: {}",
                    path.display(),
                    if init.is_err() {
                        init.unwrap_err()
                    } else {
                        write.unwrap_err()
                    }
                );
            }
        }

        if let Ok(data) = persistent.read() {
            persistent.data = data;
        }

        persistent
    }

    pub fn update(&mut self, updater: impl FnOnce(&mut R)) -> Result<(), String> {
        updater(&mut self.data);
        self.write()
    }

    fn initialize(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn read(&self) -> Result<R, String> {
        let bytes = std::fs::read(&self.path).map_err(|e| e.to_string())?;
        if self.human {
            let s = String::from_utf8(bytes).map_err(|e| e.to_string())?;
            toml::from_str(&s).map_err(|_| {
                let msg = format!(
                    "Couldn't deserialize TOML, reverting '{}' to default.",
                    self.path.display()
                );
                println!("{msg}");
                msg
            })
        } else {
            bincode::deserialize(&bytes).map_err(|_| {
                let msg = format!(
                    "Couldn't deserialize bincode, reverting '{}' to default.",
                    self.path.display()
                );
                println!("{msg}");
                msg
            })
        }
    }

    pub fn write(&self) -> Result<(), String> {
        let bytes = if self.human {
            ("# don't modify if you don't know what you're doing.\n\n".to_string()
                + &toml::to_string(&self.data).map_err(|e| e.to_string())?)
                .into_bytes()
        } else {
            bincode::serialize(&self.data).map_err(|e| e.to_string())?
        };
        std::fs::write(&self.path, bytes).map_err(|e| e.to_string())
    }
}

impl<R: Serialize + DeserializeOwned> Deref for Persistent<R> {
    type Target = R;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl<R: Serialize + DeserializeOwned> DerefMut for Persistent<R> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
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
    // pub entities: Vec<(Entity, GameEntity)>,
    pub blocks: HashMap<IVec3, Block>, // placed/broken blocks
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SavedWorld {
    pub seed: u32,
    // name, (transform, velocity, yaw, pitch)
    pub players: HashMap<String, (Vec3, Vec3, f32, f32)>,
    pub chunks: HashMap<IVec3, SavedChunk>,
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug)]
#[cfg_attr(feature = "client", derive(Component))]
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

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "client", derive(Component))]
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
    const LOOKUP_TABLE: [BlockKind; 11] = [
        BlockKind::Air,
        BlockKind::Stone,
        BlockKind::Dirt,
        BlockKind::Grass,
        BlockKind::Plank,
        BlockKind::Bedrock,
        BlockKind::Water,
        BlockKind::Sand,
        BlockKind::Wood,
        BlockKind::Leaf,
        BlockKind::Snow,
    ];

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
        matches!(self, BlockKind::Wood)
    }
    #[inline]
    pub fn from_u32(value: u32) -> BlockKind {
        Self::LOOKUP_TABLE
            .get(value as usize)
            .copied()
            .unwrap_or(BlockKind::Air)
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
        const INV_ATLAS_X: f32 = 1.0 / ATLAS_SIZE_X;
        const INV_ATLAS_Y: f32 = 1.0 / ATLAS_SIZE_Y;

        let face_idx = if self == block.direction {
            0.0
        } else if self == block.direction.get_opposite() {
            2.0
        } else {
            1.0
        };

        let x = face_idx * INV_ATLAS_X;
        let y = (block.kind as u32 - 1) as f32 * INV_ATLAS_Y;
        let y1 = y + INV_ATLAS_Y;
        let x1 = x + INV_ATLAS_X;

        let base = [
            vec2(x, y1),
            vec2(x, y),
            vec2(x1, y),
            vec2(x1, y1),
        ];

        // Optimize rotation lookup
        use Direction::*;
        match (block.direction, self) {
            (Right, Top | Bottom) => base,
            (Right, Back) => [base[3], base[0], base[1], base[2]],
            (Right, _) => [base[1], base[2], base[3], base[0]],
            (Top, Front | Back) => base,
            (Top, Left) => [base[3], base[0], base[1], base[2]],
            (Top, _) => [base[1], base[2], base[3], base[0]],
            (Front, Right | Left) => base,
            (Front, Bottom) => [base[3], base[0], base[1], base[2]],
            (Front, _) => [base[1], base[2], base[3], base[0]],
            (Left, Top | Bottom) => [base[2], base[3], base[0], base[1]],
            (Left, Back) => [base[1], base[2], base[3], base[0]],
            (Left, _) => [base[3], base[0], base[1], base[2]],
            (Bottom, Front | Back) => [base[2], base[3], base[0], base[1]],
            (Bottom, Left) => [base[1], base[2], base[3], base[0]],
            (Bottom, _) => [base[3], base[0], base[1], base[2]],
            (Back, Right | Left) => [base[2], base[3], base[0], base[1]],
            (Back, Bottom) => [base[1], base[2], base[3], base[0]],
            (Back, _) => [base[3], base[0], base[1], base[2]],
        }
    }
}
