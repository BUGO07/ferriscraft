use std::collections::{HashMap, hash_map::Entry};

use bevy::prelude::*;
use bevy_inspector_egui::egui::lerp;
use noiz::{
    Noise, NoiseFunction, SampleableFor,
    prelude::{
        FractalLayers, Normed, Persistence,
        common_noise::{Fbm, Perlin, Simplex},
    },
    rng::NoiseRng,
};
use serde::{Deserialize, Serialize};

use crate::{
    CHUNK_HEIGHT, CHUNK_SIZE, GameInfo, SEA_LEVEL,
    mesher::{Chunk, ChunkEntity, SavedChunk, SavedWorld},
};

#[inline]
pub fn vec3_to_index(pos: IVec3) -> usize {
    (pos.x + pos.y * CHUNK_SIZE + pos.z * CHUNK_SIZE * CHUNK_HEIGHT) as usize
}

#[inline]
pub fn index_to_vec3(index: usize) -> IVec3 {
    ivec3(
        index as i32 % CHUNK_SIZE,
        (index as i32 / CHUNK_SIZE) % CHUNK_HEIGHT,
        index as i32 / (CHUNK_SIZE * CHUNK_HEIGHT),
    )
}

#[inline]
pub fn generate_indices(vertex_count: usize) -> Vec<u32> {
    let indices_count = vertex_count / 4;
    let mut indices = Vec::<u32>::with_capacity(indices_count * 6);
    (0..indices_count).for_each(|vert_index| {
        let vert_index = vert_index as u32 * 4u32;
        indices.push(vert_index);
        indices.push(vert_index + 1);
        indices.push(vert_index + 2);
        indices.push(vert_index);
        indices.push(vert_index + 2);
        indices.push(vert_index + 3);
    });
    indices
}

#[inline]
pub fn make_vertex_u32(pos: IVec3, _ao: u32, normal: u32, block_type: u32) -> u32 {
    (pos.x as u32 & 0x3F)               // 6 bits for x (0-5)
        | ((pos.y as u32 & 0x1FF) << 6) // 9 bits for y (6-14)
        | ((pos.z as u32 & 0x3F) << 15) // 6 bits for z (15-20)
        | ((normal & 0x7) << 21)        // 3 bits for normal (21-23)
        | ((block_type & 0xFF) << 24) // 8 bits for block_type (24-31)
}

#[inline]
// pos, ao, normal, block_type
pub fn get_vertex_u32(vertex: u32) -> ([f32; 3], u32, [f32; 3], u32) {
    let x = (vertex & 0x3F) as f32;
    let y = ((vertex >> 6) & 0x1FF) as f32;
    let z = ((vertex >> 15) & 0x3F) as f32;
    let normal_index = (vertex >> 21) & 0x7;
    let block_type = (vertex >> 24) & 0xFF;

    (
        [x, y, z],
        0, // ao (not yet)
        Direction::NORMALS[normal_index as usize],
        block_type,
    )
}

#[inline]
pub fn noise<T: NoiseFunction<Vec2, Output = f32>>(noise: Noise<T>, pos: Vec2) -> f32 {
    let n: f32 = noise.sample(pos);
    (n + 1.0) / 2.0
}

#[inline]
// max_y, biome
pub fn terrain_noise(pos: Vec2, seed: u32) -> (i32, f32) {
    const FBM_OCTAVES: u32 = 4;
    const FBM_PERSISTENCE: f32 = 0.5;
    const FBM_LACUNARITY: f32 = 2.0;
    const FBM_BASE_FREQUENCY: f32 = 0.00200;

    const FBM_BIOME_FREQUENCY: f32 = 0.0001;
    const BIOME_OCTAVES: u32 = 3;
    const BIOME_PERSISTENCE: f32 = 0.6;

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

    let fbm_noise_generator = Noise {
        noise: Fbm::<Simplex>::new(
            Normed::default(),
            Persistence(FBM_PERSISTENCE),
            FractalLayers {
                amount: FBM_OCTAVES,
                lacunarity: FBM_LACUNARITY,
                ..Default::default()
            },
        ),
        frequency: FBM_BASE_FREQUENCY,
        seed: NoiseRng(seed),
    };

    let raw_fbm_value: f32 = fbm_noise_generator.sample(pos);

    let normalized_fbm_value = (raw_fbm_value + 1.0) / 2.0;

    let biome_noise_generator = Noise {
        noise: Fbm::<Simplex>::new(
            Normed::default(),
            Persistence(BIOME_PERSISTENCE),
            FractalLayers {
                amount: BIOME_OCTAVES,
                lacunarity: FBM_LACUNARITY,
                ..Default::default()
            },
        ),
        frequency: FBM_BIOME_FREQUENCY,
        seed: NoiseRng(seed + 1),
    };

    let raw_biome_value: f32 = biome_noise_generator.sample(pos);
    let normalized_biome_value = (raw_biome_value + 1.0) / 2.0;

    let current_min_height: f32;
    let current_max_height: f32;
    let current_flattening_exponent: f32;

    if normalized_biome_value < OCEAN_PLAINS_THRESHOLD {
        let t = normalized_biome_value / OCEAN_PLAINS_THRESHOLD;
        current_min_height = lerp(OCEAN_MIN_HEIGHT..=PLAINS_MIN_HEIGHT, t);
        current_max_height = lerp(OCEAN_MAX_HEIGHT..=PLAINS_MAX_HEIGHT, t);
        current_flattening_exponent =
            lerp(OCEAN_FLATTENING_EXPONENT..=PLAINS_FLATTENING_EXPONENT, t);
    } else if normalized_biome_value < PLAINS_MOUNTAIN_THRESHOLD {
        let t = (normalized_biome_value - OCEAN_PLAINS_THRESHOLD)
            / (PLAINS_MOUNTAIN_THRESHOLD - OCEAN_PLAINS_THRESHOLD);
        current_min_height = lerp(PLAINS_MIN_HEIGHT..=MOUNTAIN_MIN_HEIGHT, t);
        current_max_height = lerp(PLAINS_MAX_HEIGHT..=MOUNTAIN_MAX_HEIGHT, t);
        current_flattening_exponent =
            lerp(PLAINS_FLATTENING_EXPONENT..=MOUNTAIN_FLATTENING_EXPONENT, t);
    } else {
        current_min_height = MOUNTAIN_MIN_HEIGHT;
        current_max_height = MOUNTAIN_MAX_HEIGHT;
        current_flattening_exponent = MOUNTAIN_FLATTENING_EXPONENT;
    }

    let biased_noise = normalized_fbm_value.powf(current_flattening_exponent);

    let height_range = current_max_height - current_min_height;

    (
        (current_min_height + (biased_noise * height_range)) as i32,
        normalized_biome_value,
    )

    // height.max(1.0).min((CHUNK_HEIGHT - 1) as f32) as i32

    // .powf(2.0)
    //     / 3.0
    //     * (CHUNK_HEIGHT) as f32) as i32
    // + SEA_LEVEL
    // [-1.0 .. 1.0] -> [0.0 .. 2.0] -> [0 .. CHUNK_HEIGHT - SEA_LEVEL] + SEA_LEVEL
}

#[inline]
pub fn tree_noise(pos: Vec2, seed: u32) -> f32 {
    noise(
        Noise {
            noise: Perlin::default(),
            frequency: 0.069,
            seed: NoiseRng(seed),
        },
        pos,
    )
}

#[inline]
pub fn ferris_noise(pos: Vec2, seed: u32) -> f32 {
    noise(
        Noise {
            noise: Perlin::default(),
            frequency: 0.42,
            seed: NoiseRng(seed),
        },
        pos,
    )
}

pub fn update_chunk(
    commands: &mut Commands,
    chunks: &Query<(Entity, &Transform), With<ChunkEntity>>,
    pos: IVec3,
) {
    for (entity, transform) in chunks.iter() {
        if (transform.translation / CHUNK_SIZE as f32).as_ivec3() == pos {
            commands
                .entity(entity)
                .try_remove::<ChunkEntity>()
                .try_insert(ChunkEntity);
        }
    }
}

// very shitty way but it works
pub fn place_block(
    commands: &mut Commands,
    saved_chunks: &mut SavedWorld,
    chunk: &mut Chunk,
    chunk_pos: IVec3,
    chunks: &Query<(Entity, &Transform), With<ChunkEntity>>,
    pos: IVec3,
    block: Block,
) {
    chunk.blocks[vec3_to_index(pos)] = block;
    if let Entry::Vacant(e) = saved_chunks.1.entry(chunk_pos) {
        e.insert(SavedChunk {
            pos: chunk_pos,
            blocks: HashMap::from([(pos, block)]),
            entities: chunk.entities.clone(),
        });
    } else {
        let old_save = saved_chunks.1.get_mut(&chunk_pos).unwrap();
        old_save.blocks.insert(pos, block);
    }
    if pos.x == 0 {
        update_chunk(commands, chunks, chunk_pos - ivec3(1, 0, 0));
    }
    if pos.x == CHUNK_SIZE - 1 {
        update_chunk(commands, chunks, chunk_pos + ivec3(1, 0, 0));
    }
    if pos.z == 0 {
        update_chunk(commands, chunks, chunk_pos - ivec3(0, 0, 1));
    }
    if pos.z == CHUNK_SIZE - 1 {
        update_chunk(commands, chunks, chunk_pos + ivec3(0, 0, 1));
    }
    update_chunk(commands, chunks, chunk_pos);
}

#[derive(Debug)]
pub struct RayHit {
    pub global_position: IVec3,
    pub chunk_pos: IVec3,
    pub local_pos: IVec3,
    pub normal: IVec3,
    pub distance: f32,
}

pub fn ray_cast(
    game_info: &GameInfo,
    ray_origin: Vec3,
    ray_direction: Vec3,
    max_distance: f32,
) -> Option<RayHit> {
    let ray_direction = ray_direction.normalize();

    let mut current_block_pos = ray_origin.floor();

    let step_x = if ray_direction.x >= 0.0 { 1 } else { -1 };
    let step_y = if ray_direction.y >= 0.0 { 1 } else { -1 };
    let step_z = if ray_direction.z >= 0.0 { 1 } else { -1 };

    let t_delta_x = if ray_direction.x == 0.0 {
        f32::INFINITY
    } else {
        (1.0 / ray_direction.x).abs()
    };
    let t_delta_y = if ray_direction.y == 0.0 {
        f32::INFINITY
    } else {
        (1.0 / ray_direction.y).abs()
    };
    let t_delta_z = if ray_direction.z == 0.0 {
        f32::INFINITY
    } else {
        (1.0 / ray_direction.z).abs()
    };

    let mut t_max_x = if ray_direction.x >= 0.0 {
        (current_block_pos.x + 1.0 - ray_origin.x) / ray_direction.x
    } else {
        (current_block_pos.x - ray_origin.x) / ray_direction.x
    };
    if ray_direction.x == 0.0 {
        t_max_x = f32::INFINITY;
    }

    let mut t_max_y = if ray_direction.y >= 0.0 {
        (current_block_pos.y + 1.0 - ray_origin.y) / ray_direction.y
    } else {
        (current_block_pos.y - ray_origin.y) / ray_direction.y
    };
    if ray_direction.y == 0.0 {
        t_max_y = f32::INFINITY;
    }

    let mut t_max_z = if ray_direction.z >= 0.0 {
        (current_block_pos.z + 1.0 - ray_origin.z) / ray_direction.z
    } else {
        (current_block_pos.z - ray_origin.z) / ray_direction.z
    };
    if ray_direction.z == 0.0 {
        t_max_z = f32::INFINITY;
    }

    let mut current_distance = 0.0;
    let mut normal;

    while current_distance <= max_distance {
        if t_max_x < t_max_y && t_max_x < t_max_z {
            current_block_pos.x += step_x as f32;
            current_distance = t_max_x;
            t_max_x += t_delta_x;
            normal = IVec3::new(-step_x, 0, 0);
        } else if t_max_y < t_max_z {
            current_block_pos.y += step_y as f32;
            current_distance = t_max_y;
            t_max_y += t_delta_y;
            normal = IVec3::new(0, -step_y, 0);
        } else {
            current_block_pos.z += step_z as f32;
            current_distance = t_max_z;
            t_max_z += t_delta_z;
            normal = IVec3::new(0, 0, -step_z);
        }

        let chunk_pos = ivec3(
            current_block_pos.x.div_euclid(CHUNK_SIZE as f32) as i32,
            0,
            current_block_pos.z.div_euclid(CHUNK_SIZE as f32) as i32,
        );

        let local_block_pos = vec3(
            current_block_pos.x.rem_euclid(CHUNK_SIZE as f32),
            current_block_pos.y,
            current_block_pos.z.rem_euclid(CHUNK_SIZE as f32),
        )
        .as_ivec3();

        let chunks_guard = game_info.chunks.read().unwrap();

        if let Some(chunk) = chunks_guard.get(&chunk_pos) {
            let block_index = vec3_to_index(local_block_pos);

            if block_index < chunk.blocks.len() && (0..CHUNK_HEIGHT).contains(&local_block_pos.y) {
                let block = &chunk.blocks[block_index];

                if block.kind.is_solid() {
                    return Some(RayHit {
                        global_position: current_block_pos.as_ivec3(),
                        chunk_pos,
                        local_pos: local_block_pos,
                        normal,
                        distance: current_distance,
                    });
                }
            }
        }
    }

    None
}

#[derive(
    Component, Clone, Copy, Default, Debug, PartialEq, Eq, Reflect, Serialize, Deserialize,
)]
pub struct Block {
    pub kind: BlockKind,
}

impl Block {
    pub const DEFAULT: Self = Self::AIR;
    pub const AIR: Self = Self {
        kind: BlockKind::Air,
    };
    pub const STONE: Self = Self {
        kind: BlockKind::Stone,
    };
    pub const DIRT: Self = Self {
        kind: BlockKind::Dirt,
    };
    pub const GRASS: Self = Self {
        kind: BlockKind::Grass,
    };
    pub const PLANK: Self = Self {
        kind: BlockKind::Plank,
    };
    pub const BEDROCK: Self = Self {
        kind: BlockKind::Bedrock,
    };
    pub const WATER: Self = Self {
        kind: BlockKind::Water,
    };
    pub const SAND: Self = Self {
        kind: BlockKind::Sand,
    };
    pub const WOOD: Self = Self {
        kind: BlockKind::Wood,
    };
    pub const LEAF: Self = Self {
        kind: BlockKind::Leaf,
    };
    pub const SNOW: Self = Self {
        kind: BlockKind::Snow,
    };
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Reflect, Serialize, Deserialize)]
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

impl BlockKind {
    pub fn is_solid(self) -> bool {
        self != BlockKind::Air && self != BlockKind::Water
    }

    pub fn is_air(self) -> bool {
        self == BlockKind::Air
    }

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

// * using textures now
// pub fn kind2color(kind: BlockKind) -> Color {
//     let color: u32 = match kind {
//         BlockKind::Air => 0x00000000,
//         BlockKind::Stone => 0xFFA39E99,
//         BlockKind::Dirt => 0xFF915E34,
//         BlockKind::Grass => 0xFF119C13,
//         BlockKind::Plank => 0xFFA39E99,
//     };
//     let b = color & 0xFF;
//     let g = (color >> 8) & 0xFF;
//     let r = (color >> 16) & 0xFF;
//     let a = (color >> 24) & 0xFF;
//     Color::srgba_u8(r as u8, g as u8, b as u8, a as u8)
// }

#[repr(u32)]
#[derive(Clone, Copy)]
pub enum Direction {
    West,
    East,
    Bottom,
    Top,
    South,
    North,
}

impl Direction {
    pub const NORMALS: &[[f32; 3]; 6] = &[
        [-1.0, 0.0, 0.0], // West
        [1.0, 0.0, 0.0],  // East
        [0.0, -1.0, 0.0], // Bottom
        [0.0, 1.0, 0.0],  // Top
        [0.0, 0.0, -1.0], // South
        [0.0, 0.0, 1.0],  // North
    ];

    pub fn get_normal(self) -> u32 {
        self as u32
    }

    pub fn get_normal3d(self) -> [f32; 3] {
        Self::NORMALS[self.get_normal() as usize]
    }

    pub fn get_opposite(self) -> Self {
        match self {
            Direction::West => Direction::East,
            Direction::East => Direction::West,
            Direction::Bottom => Direction::Top,
            Direction::Top => Direction::Bottom,
            Direction::South => Direction::North,
            Direction::North => Direction::South,
        }
    }
}

pub struct Quad {
    pub direction: Direction,
    pub corners: [[i32; 3]; 4],
}

impl Quad {
    #[inline]
    // TODO make water be smaller than 1x1x1
    pub fn from_direction(direction: Direction, pos: IVec3, size: IVec3) -> Self {
        let corners = match direction {
            Direction::West => [
                [pos.x, pos.y, pos.z],
                [pos.x, pos.y, pos.z + size.z],
                [pos.x, pos.y + size.y, pos.z + size.z],
                [pos.x, pos.y + size.y, pos.z],
            ],
            Direction::East => [
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
            Direction::South => [
                [pos.x, pos.y, pos.z],
                [pos.x, pos.y + size.y, pos.z],
                [pos.x + size.x, pos.y + size.y, pos.z],
                [pos.x + size.x, pos.y, pos.z],
            ],
            Direction::North => [
                [pos.x + size.x, pos.y, pos.z],
                [pos.x + size.x, pos.y + size.y, pos.z],
                [pos.x, pos.y + size.y, pos.z],
                [pos.x, pos.y, pos.z],
            ],
        };

        Self { corners, direction }
    }
}

// ai-generated tree lmao
pub const TREE_OBJECT: [[[Block; 5]; 5]; 7] = [
    [
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
        [Block::AIR, Block::AIR, Block::WOOD, Block::AIR, Block::AIR],
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
    ],
    [
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
        [Block::AIR, Block::AIR, Block::WOOD, Block::AIR, Block::AIR],
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
    ],
    [
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
        [Block::AIR, Block::AIR, Block::WOOD, Block::AIR, Block::AIR],
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
    ],
    [
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
        [
            Block::AIR,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::AIR,
        ],
        [
            Block::AIR,
            Block::LEAF,
            Block::WOOD,
            Block::LEAF,
            Block::AIR,
        ],
        [
            Block::AIR,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::AIR,
        ],
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
    ],
    [
        [
            Block::AIR,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::AIR,
        ],
        [
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
        ],
        [
            Block::LEAF,
            Block::LEAF,
            Block::WOOD,
            Block::LEAF,
            Block::LEAF,
        ],
        [
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
        ],
        [
            Block::AIR,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::AIR,
        ],
    ],
    [
        [
            Block::AIR,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::AIR,
        ],
        [
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
        ],
        [
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
        ],
        [
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
        ],
        [
            Block::AIR,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::AIR,
        ],
    ],
    [
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
        [
            Block::AIR,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::AIR,
        ],
        [
            Block::AIR,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::AIR,
        ],
        [
            Block::AIR,
            Block::LEAF,
            Block::LEAF,
            Block::LEAF,
            Block::AIR,
        ],
        [Block::AIR, Block::AIR, Block::AIR, Block::AIR, Block::AIR],
    ],
];
