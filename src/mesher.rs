use std::collections::HashMap;

use bevy::prelude::*;

use crate::{
    CHUNK_HEIGHT, CHUNK_SIZE,
    utils::{
        Direction, Quad, generate_block_at, generate_indices, index_to_vec3, make_vertex_u32,
        vec3_to_index,
    },
    world::{Block, Chunk},
};

#[derive(Default)]
pub struct ChunkMesh {
    pub indices: Vec<u32>,
    pub vertices: Vec<u32>,
    pub uvs: Vec<Vec2>,
}

fn push_face(mesh: &mut ChunkMesh, dir: Direction, vpos: IVec3, block: Block) {
    let quad = Quad::from_direction(dir, vpos, IVec3::ONE);

    for (i, corner) in quad.corners.into_iter().enumerate() {
        mesh.vertices
            .push(make_vertex_u32(IVec3::from_array(corner), dir, block.kind));

        mesh.uvs.push(dir.get_uvs(block)[i]);
    }
}

pub fn build_chunk_mesh(
    chunks_refs: &Chunk,
    chunks: &HashMap<IVec3, Chunk>,
    seed: u32,
) -> Option<ChunkMesh> {
    let mut mesh = ChunkMesh::default();
    for i in 0..CHUNK_SIZE * CHUNK_HEIGHT * CHUNK_SIZE {
        let local = index_to_vec3(i as usize);
        let (current, back, left, down) = chunks_refs.get_adjacent_blocks(local, chunks, seed);
        match !current.kind.is_air() {
            true => {
                if left.kind.is_air() {
                    push_face(&mut mesh, Direction::Left, local, current);
                }
                if back.kind.is_air() {
                    push_face(&mut mesh, Direction::Back, local, current);
                }
                if down.kind.is_air() {
                    push_face(&mut mesh, Direction::Bottom, local, current);
                }
            }
            false => {
                if !left.kind.is_air() {
                    push_face(&mut mesh, Direction::Right, local, left);
                }
                if !back.kind.is_air() {
                    push_face(&mut mesh, Direction::Front, local, back);
                }
                if !down.kind.is_air() {
                    push_face(&mut mesh, Direction::Top, local, down);
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

impl Chunk {
    pub fn new(pos: IVec3) -> Self {
        Chunk {
            pos,
            entities: Vec::new(),
            blocks: vec![Block::DEFAULT; (CHUNK_SIZE * CHUNK_HEIGHT * CHUNK_SIZE) as usize],
        }
    }
    pub fn get_block(&self, pos: IVec3) -> &Block {
        let index = vec3_to_index(pos);
        if index < self.blocks.len() {
            &self.blocks[index]
        } else {
            &Block::AIR
        }
    }

    // takes relative block position
    // returns chunk pos
    pub fn get_relative_chunk(&self, pos: IVec3) -> Option<IVec3> {
        if !(0..CHUNK_HEIGHT).contains(&pos.y) {
            return None;
        }

        let mut chunk_pos = self.pos;

        if pos.x < 0 {
            chunk_pos.x -= 1;
        } else if pos.x >= CHUNK_SIZE {
            chunk_pos.x += 1;
        }

        if pos.z < 0 {
            chunk_pos.z -= 1;
        } else if pos.z >= CHUNK_SIZE {
            chunk_pos.z += 1;
        }

        Some(chunk_pos)
    }

    pub fn get_adjacent_blocks(
        &self,
        pos: IVec3,
        chunks: &HashMap<IVec3, Chunk>,
        seed: u32,
        // current, back, left, down
    ) -> (Block, Block, Block, Block) {
        let current = self.get_block(pos);

        let get_block = |pos: IVec3| -> Block {
            let mut x = pos.x;
            let y = pos.y;
            let mut z = pos.z;

            if !(0..CHUNK_HEIGHT).contains(&y) {
                return Block::AIR;
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

            if let Some(chunk) = chunks.get(&chunk_pos) {
                chunk.blocks[vec3_to_index(ivec3(x, y, z))]
            } else {
                generate_block_at(chunk_pos * CHUNK_SIZE + ivec3(x, y, z), seed)
            }
        };

        let back = get_block(pos + ivec3(0, 0, -1));
        let left = get_block(pos + ivec3(-1, 0, 0));
        let down = get_block(pos + ivec3(0, -1, 0));
        (*current, back, left, down)
    }
}
