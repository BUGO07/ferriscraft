use std::collections::{HashMap, hash_map::Entry};

use bevy::{prelude::*, window::CursorGrabMode};
use noiz::{
    Noise, NoiseFunction, SampleableFor,
    prelude::common_noise::{Fbm, Perlin, Simplex},
};
use serde::{Deserialize, Serialize};

use crate::{
    CHUNK_HEIGHT, CHUNK_SIZE, GameInfo, SEA_LEVEL,
    world::{Block, BlockKind, Chunk, ChunkMarker, SavedChunk},
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
pub fn aabb_collision(pos1: Vec3, size1: Vec3, pos2: Vec3, size2: Vec3) -> bool {
    let min1 = pos1;
    let max1 = pos1 + size1;

    let min2 = pos2;
    let max2 = pos2 + size2;

    min1.x < max2.x
        && max1.x > min2.x
        && min1.y < max2.y
        && max1.y > min2.y
        && min1.z < max2.z
        && max1.z > min2.z
}

#[derive(Default, Clone, Copy)]
pub struct NoiseFunctions {
    pub terrain: Noise<Fbm<Simplex>>,
    pub biome: Noise<Fbm<Simplex>>,
    pub ferris: Noise<Perlin>,
    pub tree: Noise<Perlin>,
}

#[inline]
pub fn noise<T: NoiseFunction<Vec2, Output = f32>>(noise: Noise<T>, pos: Vec2) -> f32 {
    let n: f32 = noise.sample(pos);
    (n + 1.0) / 2.0
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
pub fn toggle_grab_cursor(window: &mut Window) {
    if window.cursor_options.grab_mode == CursorGrabMode::None {
        window.cursor_options.grab_mode = CursorGrabMode::Locked;
        window.cursor_options.visible = false;
    } else {
        window.cursor_options.grab_mode = CursorGrabMode::None;
        window.cursor_options.visible = true;
    }
}

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
    saved_chunks: &mut HashMap<IVec3, SavedChunk>,
    chunk: &mut Chunk,
    chunks: &Query<(Entity, &Transform), With<ChunkMarker>>,
    pos: IVec3,
    block: Block,
) {
    chunk.blocks[vec3_to_index(pos)] = block;
    match saved_chunks.entry(chunk.pos) {
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
}

#[derive(Debug)]
pub struct RayHit {
    pub global_position: IVec3,
    pub chunk_pos: IVec3,
    pub local_pos: IVec3,
    pub normal: Direction,
    pub _block: Block,
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

    let step_x = ray_direction.x.signum();
    let step_y = ray_direction.y.signum();
    let step_z = ray_direction.z.signum();

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
            current_block_pos.x += step_x;
            current_distance = t_max_x;
            t_max_x += t_delta_x;
            normal = if step_x.is_sign_negative() {
                Direction::Right
            } else {
                Direction::Left
            };
        } else if t_max_y < t_max_z {
            current_block_pos.y += step_y;
            current_distance = t_max_y;
            t_max_y += t_delta_y;
            normal = if step_y.is_sign_negative() {
                Direction::Top
            } else {
                Direction::Bottom
            };
        } else {
            current_block_pos.z += step_z;
            current_distance = t_max_z;
            t_max_z += t_delta_z;
            normal = if step_z.is_sign_negative() {
                Direction::Front
            } else {
                Direction::Back
            };
        }

        if current_distance > max_distance {
            break;
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

        if let Some(chunk) = game_info.chunks.read().unwrap().get(&chunk_pos) {
            let block_index = vec3_to_index(local_block_pos);

            if block_index < chunk.blocks.len() && (0..CHUNK_HEIGHT).contains(&local_block_pos.y) {
                let block = chunk.blocks[block_index];

                if block.kind.is_solid() {
                    return Some(RayHit {
                        global_position: current_block_pos.as_ivec3(),
                        chunk_pos,
                        local_pos: local_block_pos,
                        normal,
                        _block: block,
                        distance: current_distance,
                    });
                }
            }
        }
    }

    None
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
    pub fn is_solid(self) -> bool {
        self != BlockKind::Air && self != BlockKind::Water
    }

    pub fn is_air(self) -> bool {
        self == BlockKind::Air
    }

    pub fn can_rotate(self) -> bool {
        match self {
            BlockKind::Wood => true,
            _ => false,
        }
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

#[repr(u8)]
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
