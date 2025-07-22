use bevy::prelude::*;
use noiz::{Noise, NoiseFunction, SampleableFor};

use crate::{CHUNK_HEIGHT, CHUNK_SIZE};

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

pub fn get_uvs(index: u32, tiles_per_row: u32) -> [f32; 2] {
    let tile_size = 1.0 / tiles_per_row as f32;
    let x = index / tiles_per_row;
    let y = index % tiles_per_row;
    [x as f32 * tile_size, y as f32 * tile_size]
}

// I DONT FUCKING KNOW HOW TO MAKE IT BETTER SO IT IS WHAT IT IS
#[inline]
pub fn noise<T: NoiseFunction<Vec2, Output = f32>>(noise: &Noise<T>, pos: Vec2) -> f32 {
    let n: f32 = noise.sample(pos);
    (n + 1.0) / 2.0
}

#[derive(Component, Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum BlockKind {
    #[default]
    Air,
    Stone,
    Dirt,
    Grass,
    Plank,
}

impl BlockKind {
    pub fn is_solid(self) -> bool {
        self != BlockKind::Air
    }

    pub fn from_u32(value: u32) -> BlockKind {
        match value {
            0 => BlockKind::Air,
            1 => BlockKind::Stone,
            2 => BlockKind::Dirt,
            3 => BlockKind::Grass,
            4 => BlockKind::Plank,
            _ => BlockKind::Air,
        }
    }
}

pub fn kind2color(kind: BlockKind) -> Color {
    let color: u32 = match kind {
        BlockKind::Air => 0x00000000,
        BlockKind::Stone => 0xFFA39E99,
        BlockKind::Dirt => 0xFF915E34,
        BlockKind::Grass => 0xFF119C13,
        BlockKind::Plank => 0xFFA39E99,
    };
    let b = color & 0xFF;
    let g = (color >> 8) & 0xFF;
    let r = (color >> 16) & 0xFF;
    let a = (color >> 24) & 0xFF;
    Color::srgba_u8(r as u8, g as u8, b as u8, a as u8)
}

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
    pub color: Color,
    pub direction: Direction,
    pub corners: [[i32; 3]; 4],
}

impl Quad {
    #[inline]
    pub fn from_direction(direction: Direction, pos: IVec3, color: Color) -> Self {
        let corners = match direction {
            Direction::West => [
                [pos.x, pos.y, pos.z],
                [pos.x, pos.y, pos.z + 1],
                [pos.x, pos.y + 1, pos.z + 1],
                [pos.x, pos.y + 1, pos.z],
            ],
            Direction::East => [
                [pos.x, pos.y + 1, pos.z],
                [pos.x, pos.y + 1, pos.z + 1],
                [pos.x, pos.y, pos.z + 1],
                [pos.x, pos.y, pos.z],
            ],
            Direction::Bottom => [
                [pos.x, pos.y, pos.z],
                [pos.x + 1, pos.y, pos.z],
                [pos.x + 1, pos.y, pos.z + 1],
                [pos.x, pos.y, pos.z + 1],
            ],
            Direction::Top => [
                [pos.x, pos.y, pos.z + 1],
                [pos.x + 1, pos.y, pos.z + 1],
                [pos.x + 1, pos.y, pos.z],
                [pos.x, pos.y, pos.z],
            ],
            Direction::South => [
                [pos.x, pos.y, pos.z],
                [pos.x, pos.y + 1, pos.z],
                [pos.x + 1, pos.y + 1, pos.z],
                [pos.x + 1, pos.y, pos.z],
            ],
            Direction::North => [
                [pos.x + 1, pos.y, pos.z],
                [pos.x + 1, pos.y + 1, pos.z],
                [pos.x, pos.y + 1, pos.z],
                [pos.x, pos.y, pos.z],
            ],
        };

        Self {
            corners,
            color,
            direction,
        }
    }
}
