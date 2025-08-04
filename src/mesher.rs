use std::collections::HashMap;

use bevy::prelude::*;

use crate::{
    CHUNK_HEIGHT, CHUNK_SIZE,
    utils::{
        Direction, NoiseFunctions, Quad, generate_block_at, index_to_vec3, terrain_noise,
        vec3_to_index,
    },
    world::{Block, Chunk},
};

#[derive(Default)]
pub struct ChunkMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

pub struct Vertex {
    pub pos: Vec3,
    pub normal: Direction,
    pub uv: Vec2,
}

impl ChunkMesh {
    pub fn build(
        mut self,
        chunk: &Chunk,
        chunks: &HashMap<IVec3, Chunk>,
        noises: &NoiseFunctions,
    ) -> Option<Self> {
        for i in 0..CHUNK_SIZE * CHUNK_HEIGHT * CHUNK_SIZE {
            let local = index_to_vec3(i as usize).as_vec3();
            let (current, back, left, down) =
                chunk.get_adjacent_blocks(local.as_ivec3(), chunks, noises);
            match !current.kind.is_air() {
                true => {
                    if left.kind.is_air() {
                        self.push_face(Direction::Left, local, current);
                    }
                    if back.kind.is_air() {
                        self.push_face(Direction::Back, local, current);
                    }
                    if down.kind.is_air() {
                        self.push_face(Direction::Bottom, local, current);
                    }
                }
                false => {
                    if !left.kind.is_air() {
                        self.push_face(Direction::Right, local, left);
                    }
                    if !back.kind.is_air() {
                        self.push_face(Direction::Front, local, back);
                    }
                    if !down.kind.is_air() {
                        self.push_face(Direction::Top, local, down);
                    }
                }
            }
        }
        if self.vertices.is_empty() {
            None
        } else {
            let count = self.vertices.len() / 4;
            let mut indices = Vec::with_capacity(count * 6);
            indices.extend((0..count).flat_map(|i| {
                let idx = i as u32 * 4;
                [idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]
            }));
            self.indices = indices;
            Some(self)
        }
    }

    #[allow(clippy::vec_init_then_push)]
    pub fn push_face(&mut self, dir: Direction, pos: Vec3, block: Block) {
        let mut quads = Vec::new();

        // * make it so stairs and other non-full blocks are possible
        // if block.kind == BlockKind::Wood {
        //     let mut quad = Quad::from_direction(
        //         dir,
        //         pos,
        //         Vec3::ONE
        //             - if matches!(dir, Direction::Top | Direction::Right | Direction::Front) {
        //                 dir.as_vec3() / 2.0
        //             } else {
        //                 Vec3::ZERO
        //             },
        //     );

        //     if matches!(dir, Direction::Top | Direction::Right | Direction::Front) {
        //         quad.translate(-dir.as_vec3() / 2.0);
        //     }

        //     quads.push(quad);
        // } else {
        quads.push(Quad::from_direction(dir, pos, Vec3::ONE));
        // }

        let uvs = dir.get_uvs(block);
        for quad in quads {
            for (i, corner) in quad.corners.into_iter().enumerate() {
                self.vertices.push(Vertex {
                    pos: Vec3::from_array(corner),
                    normal: dir,
                    uv: uvs[i],
                });
            }
        }
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
        noise_functions: &NoiseFunctions,
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
                let pos = chunk_pos * CHUNK_SIZE + ivec3(x, y, z);
                generate_block_at(pos, terrain_noise(pos.xz().as_vec2(), noise_functions).0)
            }
        };

        let back = get_block(pos + ivec3(0, 0, -1));
        let left = get_block(pos + ivec3(-1, 0, 0));
        let down = get_block(pos + ivec3(0, -1, 0));
        (*current, back, left, down)
    }
}
