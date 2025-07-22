use bevy::prelude::*;

use crate::{CHUNK_HEIGHT, CHUNK_SIZE};

#[inline]
pub fn vec3_to_index(pos: IVec3) -> usize {
    (pos.x + pos.y * CHUNK_SIZE + pos.z * CHUNK_SIZE * CHUNK_HEIGHT) as usize
}

#[inline]
pub fn index_to_vec3(index: usize) -> IVec3 {
    IVec3::new(
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
pub fn make_vertex_u32(pos: IVec3, ao: u32, normal: u32, block_type: u32) -> u32 {
    (pos.x as u32 & 0xF)       // 4 bits for x (0..15)
        | ((pos.y as u32 & 0xFF) << 4)  // 8 bits for y (0..255)
        | ((pos.z as u32 & 0xF) << 12)  // 4 bits for z (0..15)
        | ((ao & 0x7) << 16)             // 3 bits for ao
        | ((normal & 0x7) << 19)         // 3 bits for normal
        | ((block_type & 0x3FF) << 22) // 10 bits for block_type
}
