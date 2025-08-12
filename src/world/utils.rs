use std::collections::{HashMap, hash_map::Entry};

use bevy::prelude::*;
use bevy_renet::renet::RenetClient;
use ferriscraft::{Block, ClientPacket, Direction, SEA_LEVEL, SavedChunk};
use noiz::{
    Noise,
    prelude::common_noise::{Fbm, Perlin, Simplex},
};

use crate::{
    CHUNK_SIZE, GameInfo,
    utils::{noise, vec3_to_index},
    world::{Chunk, ChunkMarker},
};

pub fn update_chunk(
    commands: &mut Commands,
    chunks: &Query<(Entity, &Transform), With<ChunkMarker>>,
    pos: IVec3,
) {
    for (entity, transform) in chunks {
        if (transform.translation / CHUNK_SIZE as f32).as_ivec3() == pos {
            commands
                .entity(entity)
                .try_remove::<ChunkMarker>()
                .try_insert(ChunkMarker);
        }
    }
}

pub fn place_block(
    commands: &mut Commands,
    client: Option<ResMut<RenetClient>>,
    game_info: &GameInfo,
    chunk: &mut Chunk,
    chunks: &Query<(Entity, &Transform), With<ChunkMarker>>,
    pos: IVec3,
    block: Block,
) {
    chunk.blocks[vec3_to_index(pos)] = block;
    if let Some(saved_chunks) = &game_info.saved_chunks {
        match saved_chunks.write().unwrap().entry(chunk.pos) {
            Entry::Vacant(e) => {
                e.insert(SavedChunk {
                    blocks: HashMap::from([(pos, block)]),
                    entities: chunk.entities.clone(),
                });
            }
            Entry::Occupied(mut e) => {
                e.get_mut().blocks.insert(pos, block);
            }
        }
    }
    if pos.x == 0 {
        update_chunk(commands, chunks, chunk.pos - IVec3::X);
    }
    if pos.x == CHUNK_SIZE - 1 {
        update_chunk(commands, chunks, chunk.pos + IVec3::X);
    }
    if pos.z == 0 {
        update_chunk(commands, chunks, chunk.pos - IVec3::Z);
    }
    if pos.z == CHUNK_SIZE - 1 {
        update_chunk(commands, chunks, chunk.pos + IVec3::Z);
    }
    update_chunk(commands, chunks, chunk.pos);
    ClientPacket::PlaceBlock(chunk.pos * CHUNK_SIZE + pos, block).send(client);
}

#[derive(Default, Clone, Copy)]
pub struct NoiseFunctions {
    pub terrain: Noise<Fbm<Simplex>>,
    pub biome: Noise<Fbm<Simplex>>,
    pub ferris: Noise<Perlin>,
    pub tree: Noise<Perlin>,
}

const OCEAN_MIN_HEIGHT: f32 = SEA_LEVEL as f32 - 40.0;
const OCEAN_MAX_HEIGHT: f32 = SEA_LEVEL as f32 + 5.0;
const OCEAN_FLATTENING_EXPONENT: f32 = 4.0;
const PLAINS_MIN_HEIGHT: f32 = SEA_LEVEL as f32 + 10.0;
const PLAINS_MAX_HEIGHT: f32 = SEA_LEVEL as f32 + 40.0;
const PLAINS_FLATTENING_EXPONENT: f32 = 3.0;
const MOUNTAIN_MIN_HEIGHT: f32 = SEA_LEVEL as f32 + 50.0;
const MOUNTAIN_MAX_HEIGHT: f32 = SEA_LEVEL as f32 + 180.0;
const MOUNTAIN_FLATTENING_EXPONENT: f32 = 1.5;
const OCEAN_PLAINS_THRESHOLD: f32 = 0.4;
const PLAINS_MOUNTAIN_THRESHOLD: f32 = 0.6;

#[inline]
// max_y, biome
pub fn terrain_noise(pos: Vec2, noises: &NoiseFunctions) -> (i32, f32) {
    let terrain_fbm = noise(noises.terrain, pos);
    let biome_fbm = noise(noises.biome, pos);

    let min_height: f32;
    let max_height: f32;
    let flattening_exp: f32;

    if biome_fbm < OCEAN_PLAINS_THRESHOLD {
        let t = biome_fbm / OCEAN_PLAINS_THRESHOLD;
        min_height = OCEAN_MIN_HEIGHT.lerp(PLAINS_MIN_HEIGHT, t);
        max_height = OCEAN_MAX_HEIGHT.lerp(PLAINS_MAX_HEIGHT, t);
        flattening_exp = OCEAN_FLATTENING_EXPONENT.lerp(PLAINS_FLATTENING_EXPONENT, t);
    } else if biome_fbm < PLAINS_MOUNTAIN_THRESHOLD {
        let t = (biome_fbm - OCEAN_PLAINS_THRESHOLD)
            / (PLAINS_MOUNTAIN_THRESHOLD - OCEAN_PLAINS_THRESHOLD);
        min_height = PLAINS_MIN_HEIGHT.lerp(MOUNTAIN_MIN_HEIGHT, t);
        max_height = PLAINS_MAX_HEIGHT.lerp(MOUNTAIN_MAX_HEIGHT, t);
        flattening_exp = PLAINS_FLATTENING_EXPONENT.lerp(MOUNTAIN_FLATTENING_EXPONENT, t);
    } else {
        min_height = MOUNTAIN_MIN_HEIGHT;
        max_height = MOUNTAIN_MAX_HEIGHT;
        flattening_exp = MOUNTAIN_FLATTENING_EXPONENT;
    }

    let height = min_height + terrain_fbm.powf(flattening_exp) * (max_height - min_height);

    (height as i32, biome_fbm)
}

#[inline]
pub fn generate_block_at(pos: IVec3, max_y: i32) -> Block {
    let y = pos.y;
    if y == 0 {
        Block::BEDROCK
    } else if y < max_y {
        match y {
            _ if y > 165 => Block::SNOW,
            _ if y > 140 => Block::STONE,
            _ if y == max_y - 1 => Block::GRASS,
            _ if y >= max_y - 4 => Block::DIRT,
            _ => Block::STONE,
        }
    } else if y < SEA_LEVEL {
        Block::WATER
    } else {
        Block::AIR
    }

    // let tree_probabilty = tree_noise(pos.xz().as_vec2(), seed);

    // if tree_probabilty > 0.85 && max_y < 90 && max_y > SEA_LEVEL + 2 {
    //     for (y, tree_layer) in TREE_OBJECT.iter().enumerate() {
    //         for (z, tree_row) in tree_layer.iter().enumerate() {
    //             for (x, block) in tree_row.iter().enumerate() {
    //                 let mut tree_pos = ivec3(3 + x as i32, y as i32, 3 + z as i32);
    //                 let (local_max_y, _) = terrain_noise((pos + tree_pos).as_vec3().xz(), seed);

    //                 tree_pos.y += local_max_y;

    //                 if pos == tree_pos {
    //                     return *block;
    //                 }
    //             }
    //         }
    //     }
    // }

    // terrain_block
}
pub struct Quad {
    pub corners: [[f32; 3]; 4],
}

impl Quad {
    #[inline]
    pub fn from_direction(direction: Direction, pos: Vec3, size: Vec3) -> Self {
        let corners = match direction {
            Direction::Left => [
                [pos.x, pos.y, pos.z],
                [pos.x, pos.y, pos.z + size.z],
                [pos.x, pos.y + size.y, pos.z + size.z],
                [pos.x, pos.y + size.y, pos.z],
            ],
            Direction::Right => [
                [pos.x, pos.y + size.y, pos.z],
                [pos.x, pos.y + size.y, pos.z + size.z],
                [pos.x, pos.y, pos.z + size.z],
                [pos.x, pos.y, pos.z],
            ],
            Direction::Bottom => [
                [pos.x, pos.y, pos.z],
                [pos.x + size.x, pos.y, pos.z],
                [pos.x + size.x, pos.y, pos.z + size.z],
                [pos.x, pos.y, pos.z + size.z],
            ],
            Direction::Top => [
                [pos.x, pos.y, pos.z + size.z],
                [pos.x + size.x, pos.y, pos.z + size.z],
                [pos.x + size.x, pos.y, pos.z],
                [pos.x, pos.y, pos.z],
            ],
            Direction::Back => [
                [pos.x, pos.y, pos.z],
                [pos.x, pos.y + size.y, pos.z],
                [pos.x + size.x, pos.y + size.y, pos.z],
                [pos.x + size.x, pos.y, pos.z],
            ],
            Direction::Front => [
                [pos.x + size.x, pos.y, pos.z],
                [pos.x + size.x, pos.y + size.y, pos.z],
                [pos.x, pos.y + size.y, pos.z],
                [pos.x, pos.y, pos.z],
            ],
        };

        Self { corners }
    }

    pub fn _translate(&mut self, offset: Vec3) {
        for corner in &mut self.corners {
            corner[0] += offset.x;
            corner[1] += offset.y;
            corner[2] += offset.z;
        }
    }
}
