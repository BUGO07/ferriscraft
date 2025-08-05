use std::collections::HashMap;

use bevy::prelude::*;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::{
    CHUNK_HEIGHT, CHUNK_SIZE,
    utils::{index_to_vec3, vec3_to_index},
    world::{
        Block, Chunk,
        utils::{Direction, NoiseFunctions, Quad, generate_block_at, terrain_noise},
    },
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
        let chunk_pos = chunk.pos;

        let left_chunk = chunks.get(&(chunk_pos + IVec3::new(-1, 0, 0)));
        let back_chunk = chunks.get(&(chunk_pos + IVec3::new(0, 0, -1)));
        let down_chunk = chunks.get(&(chunk_pos + IVec3::new(0, -1, 0)));

        // parallelized (thanks rayon)
        let mesh_parts: Vec<ChunkMesh> = (0..CHUNK_SIZE * CHUNK_HEIGHT * CHUNK_SIZE)
            .into_par_iter()
            .filter_map(|i| {
                let mut local_mesh = ChunkMesh::default();

                let local = index_to_vec3(i as usize).as_vec3();
                let current = chunk.blocks[vec3_to_index(local.as_ivec3())];

                let (back, left, down) = chunk.get_adjacent_blocks(
                    local.as_ivec3(),
                    left_chunk,
                    back_chunk,
                    down_chunk,
                    noises,
                );

                if !current.kind.is_air() {
                    if left.kind.is_air() {
                        local_mesh.push_face(Direction::Left, local, current);
                    }
                    if back.kind.is_air() {
                        local_mesh.push_face(Direction::Back, local, current);
                    }
                    if down.kind.is_air() {
                        local_mesh.push_face(Direction::Bottom, local, current);
                    }
                } else {
                    if !left.kind.is_air() {
                        local_mesh.push_face(Direction::Right, local, left);
                    }
                    if !back.kind.is_air() {
                        local_mesh.push_face(Direction::Front, local, back);
                    }
                    if !down.kind.is_air() {
                        local_mesh.push_face(Direction::Top, local, down);
                    }
                }

                if local_mesh.vertices.is_empty() {
                    None
                } else {
                    Some(local_mesh)
                }
            })
            .collect();

        for mesh in mesh_parts {
            self.vertices.extend(mesh.vertices);
            self.indices.extend(mesh.indices);
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
        // }

        let uvs = dir.get_uvs(block);
        for (i, corner) in Quad::from_direction(dir, pos, Vec3::ONE)
            .corners
            .into_iter()
            .enumerate()
        {
            self.vertices.push(Vertex {
                pos: Vec3::from_array(corner),
                normal: dir,
                uv: uvs[i],
            });
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
        left_chunk: Option<&Chunk>,
        back_chunk: Option<&Chunk>,
        down_chunk: Option<&Chunk>,
        noises: &NoiseFunctions,
    ) -> (Block, Block, Block) {
        let get_block = |offset: IVec3, fallback: Option<&Chunk>| -> Block {
            let new_pos = pos + offset;
            let x = new_pos.x;
            let y = new_pos.y;
            let z = new_pos.z;

            if !(0..CHUNK_HEIGHT).contains(&y) {
                return Block::AIR;
            }

            if (0..CHUNK_SIZE).contains(&x) && (0..CHUNK_SIZE).contains(&z) {
                return *self.get_block(new_pos);
            }

            let mut chunk_pos = self.pos;
            let mut local_x = x;
            let mut local_z = z;

            if x < 0 {
                local_x += CHUNK_SIZE;
                chunk_pos.x -= 1;
            } else if x >= CHUNK_SIZE {
                local_x -= CHUNK_SIZE;
                chunk_pos.x += 1;
            }

            if z < 0 {
                local_z += CHUNK_SIZE;
                chunk_pos.z -= 1;
            } else if z >= CHUNK_SIZE {
                local_z -= CHUNK_SIZE;
                chunk_pos.z += 1;
            }

            if let Some(chunk) = fallback {
                chunk.blocks[vec3_to_index(IVec3::new(local_x, y, local_z))]
            } else {
                let world_pos = chunk_pos * CHUNK_SIZE + IVec3::new(local_x, y, local_z);
                generate_block_at(world_pos, terrain_noise(world_pos.xz().as_vec2(), noises).0)
            }
        };

        let back = get_block(IVec3::new(0, 0, -1), back_chunk);
        let left = get_block(IVec3::new(-1, 0, 0), left_chunk);
        let down = get_block(IVec3::new(0, -1, 0), down_chunk);

        (back, left, down)
    }
}
