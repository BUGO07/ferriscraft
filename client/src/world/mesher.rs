use std::collections::HashMap;

use bevy::prelude::*;
use ferriscraft::Direction;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::{
    CHUNK_HEIGHT, CHUNK_SIZE,
    utils::{index_to_vec3, vec3_to_index},
    world::{
        Block, Chunk,
        utils::{NoiseFunctions, Quad, generate_block_at, terrain_noise},
    },
};

#[derive(Default)]
pub struct ChunkMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Copy)]
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

        // parallelized (thanks rayon)
        let mesh_parts: Vec<ChunkMesh> = (0..CHUNK_SIZE * CHUNK_HEIGHT * CHUNK_SIZE)
            .into_par_iter()
            .filter_map(|i| {
                let mut local_mesh = ChunkMesh::default();

                let pos = index_to_vec3(i as usize);
                let local = pos.as_vec3();

                let current = *unsafe { chunk.blocks.get_unchecked(i as usize) };

                let (back, left, down) =
                    chunk.get_adjacent_blocks(pos, left_chunk, back_chunk, noises);

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

        for part in mesh_parts {
            for v in part.vertices {
                self.vertices.push(v);
            }
            for i in part.indices {
                self.indices.push(i + self.vertices.len() as u32);
            }
        }

        if self.vertices.is_empty() {
            None
        } else {
            self.vertices.shrink_to_fit();
            self.indices
                .extend((0..self.vertices.len() / 4).flat_map(|i| {
                    let idx = i as u32 * 4;
                    [idx, idx + 1, idx + 2, idx, idx + 2, idx + 3]
                }));
            Some(self)
        }
    }

    #[inline(always)]
    pub fn push_face(&mut self, dir: Direction, pos: Vec3, block: Block) {
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
    #[inline]
    pub fn new(pos: IVec3) -> Self {
        Chunk {
            pos,
            entities: Vec::new(),
            blocks: vec![Block::DEFAULT; (CHUNK_SIZE * CHUNK_HEIGHT * CHUNK_SIZE) as usize],
        }
    }

    // pub fn get_block(&self, pos: IVec3) -> &Block {
    //     let index = vec3_to_index(pos);
    //     if index < self.blocks.len() {
    //         &self.blocks[index]
    //     } else {
    //         &Block::AIR
    //     }
    // }

    #[inline]
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

    #[inline(always)]
    pub fn get_adjacent_blocks(
        &self,
        pos: IVec3,
        left_chunk: Option<&Chunk>,
        back_chunk: Option<&Chunk>,
        noises: &NoiseFunctions,
    ) -> (Block, Block, Block) {
        let x = pos.x;
        let y = pos.y;
        let z = pos.z;

        if !(0..CHUNK_HEIGHT).contains(&y) {
            return (Block::AIR, Block::AIR, Block::AIR);
        }

        let get_block = |dx: i32, dy: i32, dz: i32, fallback: Option<&Chunk>| -> Block {
            let nx = x + dx;
            let ny = y + dy;
            let nz = z + dz;

            if !(0..CHUNK_HEIGHT).contains(&ny) {
                return Block::AIR;
            }

            if (0..CHUNK_SIZE).contains(&nx) && (0..CHUNK_SIZE).contains(&nz) {
                return *unsafe {
                    self.blocks
                        .get_unchecked(vec3_to_index(IVec3::new(nx, ny, nz)))
                };
            }

            let mut chunk_x = self.pos.x;
            let mut chunk_z = self.pos.z;
            let mut lx = nx;
            let mut lz = nz;

            if nx < 0 {
                lx += CHUNK_SIZE;
                chunk_x -= 1;
            } else if nx >= CHUNK_SIZE {
                lx -= CHUNK_SIZE;
                chunk_x += 1;
            }

            if nz < 0 {
                lz += CHUNK_SIZE;
                chunk_z -= 1;
            } else if nz >= CHUNK_SIZE {
                lz -= CHUNK_SIZE;
                chunk_z += 1;
            }

            if let Some(chunk) = fallback {
                return *unsafe {
                    chunk
                        .blocks
                        .get_unchecked(vec3_to_index(IVec3::new(lx, ny, lz)))
                };
            }

            let world_pos = IVec3::new(chunk_x * CHUNK_SIZE + lx, ny, chunk_z * CHUNK_SIZE + lz);
            generate_block_at(world_pos, terrain_noise(world_pos.xz().as_vec2(), noises).0)
        };

        let back = get_block(0, 0, -1, back_chunk);
        let left = get_block(-1, 0, 0, left_chunk);
        let down = get_block(0, -1, 0, None);

        (back, left, down)
    }
}
