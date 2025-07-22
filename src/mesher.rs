use std::collections::HashMap;

use bevy::prelude::*;

use crate::{
    CHUNK_HEIGHT, CHUNK_SIZE, Chunk,
    utils::{generate_indices, index_to_vec3, make_vertex_u32},
};

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
    pub fn get_normal(self) -> u32 {
        self as u32
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
    // the input position is assumed to be a voxel's (0,0,0) pos
    // therefore right / up / forward are offset by 1
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

#[derive(Default)]
pub struct ChunkMesh {
    pub indices: Vec<u32>,
    pub vertices: Vec<u32>,
}

fn push_face(mesh: &mut ChunkMesh, dir: Direction, vpos: IVec3, color: Color, block_type: u32) {
    let quad = Quad::from_direction(dir, vpos, color);
    for corner in quad.corners.into_iter() {
        let corner_vec = IVec3::from_array(corner);
        let x = corner_vec.x.clamp(0, 15);
        let y = corner_vec.y.clamp(0, 255);
        let z = corner_vec.z.clamp(0, 15);
        mesh.vertices.push(make_vertex_u32(
            IVec3::new(x, y, z),
            0,
            dir.get_normal(),
            block_type,
        ));
    }
}

pub fn build_chunk_mesh(chunks_refs: &Chunk, chunks: &HashMap<IVec3, Chunk>) -> Option<ChunkMesh> {
    let mut mesh = ChunkMesh::default();
    for i in 0..CHUNK_SIZE * CHUNK_HEIGHT * CHUNK_SIZE {
        let local = index_to_vec3(i as usize);
        let (current, back, left, down) = chunks_refs.get_adjacent_blocks(local, chunks);
        match current.kind.is_solid() {
            true => {
                if !left.kind.is_solid() {
                    push_face(
                        &mut mesh,
                        Direction::West,
                        local,
                        Color::srgba_u8(0, 255, 0, 255),
                        current.kind as u32,
                    );
                }
                if !back.kind.is_solid() {
                    push_face(
                        &mut mesh,
                        Direction::South,
                        local,
                        Color::srgba_u8(0, 255, 0, 255),
                        current.kind as u32,
                    );
                }
                if !down.kind.is_solid() {
                    push_face(
                        &mut mesh,
                        Direction::Bottom,
                        local,
                        Color::srgba_u8(0, 255, 0, 255),
                        current.kind as u32,
                    );
                }
            }
            false => {
                if left.kind.is_solid() {
                    push_face(
                        &mut mesh,
                        Direction::East,
                        local,
                        Color::srgba_u8(0, 255, 0, 255),
                        left.kind as u32,
                    );
                }
                if back.kind.is_solid() {
                    push_face(
                        &mut mesh,
                        Direction::North,
                        local,
                        Color::srgba_u8(0, 255, 0, 255),
                        back.kind as u32,
                    );
                }
                if down.kind.is_solid() {
                    push_face(
                        &mut mesh,
                        Direction::Top,
                        local,
                        Color::srgba_u8(0, 255, 0, 255),
                        down.kind as u32,
                    );
                }
            }
        }
    }
    if mesh.vertices.is_empty() {
        None
    } else {
        mesh.indices = generate_indices(mesh.vertices.len());
        Some(mesh)
    }
}
