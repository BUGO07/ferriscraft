use std::collections::HashMap;

use bevy::prelude::*;

use crate::{
    CHUNK_HEIGHT, CHUNK_SIZE,
    utils::{
        Block, BlockKind, Direction, Quad, generate_indices, get_uvs, index_to_vec3,
        make_vertex_u32, vec3_to_index,
    },
};

#[derive(Default)]
pub struct ChunkMesh {
    pub indices: Vec<u32>,
    pub vertices: Vec<u32>,
    pub uvs: Vec<[f32; 2]>,
}

#[derive(Component, Clone)]
pub struct Chunk {
    pub pos: IVec3,
    pub blocks: Vec<Block>,
}

impl Chunk {
    pub fn get_block(&self, pos: IVec3) -> &Block {
        let index = vec3_to_index(pos);
        if index < self.blocks.len() {
            &self.blocks[index]
        } else {
            &Block {
                kind: BlockKind::Air,
            }
        }
    }

    pub fn get_adjacent_blocks(
        &self,
        pos: IVec3,
        chunks: &HashMap<IVec3, Chunk>,
        // current, back, left, down
    ) -> (Block, Block, Block, Block) {
        let current = self.get_block(pos);

        let get_block = |pos: IVec3| -> Option<Block> {
            let mut x = pos.x;
            let y = pos.y;
            let mut z = pos.z;

            if !(0..CHUNK_HEIGHT).contains(&y) {
                return None;
            }

            let mut chunk_pos = self.pos;

            if x < 0 {
                x += CHUNK_SIZE;
                chunk_pos.x -= 1;
            } else if x >= CHUNK_SIZE {
                x -= CHUNK_SIZE;
                chunk_pos.x += 1;
            }

            if z < 0 {
                z += CHUNK_SIZE;
                chunk_pos.z -= 1;
            } else if z >= CHUNK_SIZE {
                z -= CHUNK_SIZE;
                chunk_pos.z += 1;
            }

            let chunk = chunks.get(&chunk_pos)?;
            chunk.blocks.get(vec3_to_index(ivec3(x, y, z))).copied()
        };

        let back = get_block(pos + ivec3(0, 0, -1)).unwrap_or_default();
        let left = get_block(pos + ivec3(-1, 0, 0)).unwrap_or_default();
        let down = get_block(pos + ivec3(0, -1, 0)).unwrap_or_default();
        (*current, back, left, down)
    }

    pub fn get_von_neumann(&self, pos: IVec3) -> Vec<(Direction, &Block)> {
        vec![
            (Direction::South, self.get_block(pos + ivec3(0, 0, -1))),
            (Direction::North, self.get_block(pos + ivec3(0, 0, 1))),
            (Direction::Bottom, self.get_block(pos + ivec3(0, -1, 0))),
            (Direction::Top, self.get_block(pos + ivec3(0, 1, 0))),
            (Direction::West, self.get_block(pos + ivec3(-1, 0, 0))),
            (Direction::East, self.get_block(pos + ivec3(1, 0, 0))),
        ]
    }
}

fn push_face(mesh: &mut ChunkMesh, dir: Direction, vpos: IVec3, color: Color, block_type: u32) {
    let quad = Quad::from_direction(dir, vpos, color);

    let uv_origin = get_uvs(block_type - 1, 2);
    let tile_size = 0.5;

    let uv_corners = [
        [uv_origin[0], uv_origin[1]],
        [uv_origin[0] + tile_size, uv_origin[1]],
        [uv_origin[0] + tile_size, uv_origin[1] + tile_size],
        [uv_origin[0], uv_origin[1] + tile_size],
    ];

    for (i, corner) in quad.corners.into_iter().enumerate() {
        mesh.vertices.push(make_vertex_u32(
            IVec3::from_array(corner),
            0,
            dir.get_normal(),
            block_type,
        ));

        mesh.uvs.push(uv_corners[i]);
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
