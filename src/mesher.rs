use std::{collections::HashMap, hash::Hash};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
    CHUNK_HEIGHT, CHUNK_SIZE,
    utils::{
        Block, Direction, Quad, generate_indices, index_to_vec3, make_vertex_u32, vec3_to_index,
    },
};

#[derive(Default)]
pub struct ChunkMesh {
    pub indices: Vec<u32>,
    pub vertices: Vec<u32>,
    pub uvs: Vec<[f32; 2]>,
}

#[derive(Component)]
pub struct ChunkEntity;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GameEntityKind {
    Ferris,
}

#[derive(Component, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct GameEntity {
    pub kind: GameEntityKind,
    pub pos: Vec3,
    pub rot: f32,
}

#[derive(Clone)]
pub struct Chunk {
    pub pos: IVec3,
    pub entities: Vec<(Entity, GameEntity)>,
    pub blocks: Vec<Block>,
}

#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct SavedWorld(pub u32, pub HashMap<IVec3, SavedChunk>);

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct SavedChunk {
    pub pos: IVec3,
    pub entities: Vec<(Entity, GameEntity)>,
    pub blocks: HashMap<IVec3, Block>, // placed/broken blocks
}

fn push_face(mesh: &mut ChunkMesh, dir: Direction, vpos: IVec3, block: Block) {
    let quad = Quad::from_direction(dir, vpos, IVec3::ONE);

    for (i, corner) in quad.corners.into_iter().enumerate() {
        mesh.vertices.push(make_vertex_u32(
            IVec3::from_array(corner),
            0,
            dir.get_normal(),
            block.kind as u32,
        ));

        mesh.uvs.push(dir.get_uvs(block)[i]);
    }
}

pub fn build_chunk_mesh(chunks_refs: &Chunk, chunks: &HashMap<IVec3, Chunk>) -> Option<ChunkMesh> {
    let mut mesh = ChunkMesh::default();
    for i in 0..CHUNK_SIZE * CHUNK_HEIGHT * CHUNK_SIZE {
        let local = index_to_vec3(i as usize);
        let (current, back, left, down) = chunks_refs.get_adjacent_blocks(local, chunks);
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
            blocks: vec![Block::default(); (CHUNK_SIZE * CHUNK_HEIGHT * CHUNK_SIZE) as usize],
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
}
