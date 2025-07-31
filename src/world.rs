use std::{collections::HashMap, path::Path};

use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::{
        mesh::{Indices, PrimitiveTopology},
        primitives::Aabb,
    },
    tasks::{AsyncComputeTaskPool, Task, futures_lite::future},
};
use bevy_persistent::{Persistent, StorageFormat};
use serde::{Deserialize, Serialize};

use crate::{
    CHUNK_HEIGHT, CHUNK_SIZE, GameInfo, GameSettings, SEA_LEVEL,
    mesher::{ChunkMesh, build_chunk_mesh},
    player::Player,
    utils::{
        Direction, TREE_OBJECT, ferris_noise, generate_block_at, get_vertex_u32, terrain_noise,
        tree_noise, vec3_to_index,
    },
};

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(
            Persistent::<SavedWorld>::builder()
                .name("saved world")
                .format(StorageFormat::Bincode)
                .path(Path::new("saves").join("world.ferris"))
                .default(SavedWorld(rand::random(), HashMap::new()))
                .build()
                .unwrap(),
        )
        .add_systems(
            Update,
            (
                handle_chunk_gen,
                handle_mesh_gen,
                handle_chunk_despawn
                    .run_if(|game_settings: Res<GameSettings>| game_settings.despawn_chunks),
                process_tasks,
            ),
        );
    }
}

#[derive(Component)]
pub struct ChunkMarker;

#[derive(Clone)]
pub struct Chunk {
    pub pos: IVec3,
    pub entities: Vec<(Entity, GameEntity)>,
    pub blocks: Vec<Block>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct SavedChunk {
    pub pos: IVec3,
    pub entities: Vec<(Entity, GameEntity)>,
    pub blocks: HashMap<IVec3, Block>, // placed/broken blocks
}

#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct SavedWorld(pub u32, pub HashMap<IVec3, SavedChunk>);

#[derive(Component, Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    pub kind: BlockKind,
    pub direction: Direction,
}

#[derive(Component, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct GameEntity {
    pub kind: GameEntityKind,
    pub pos: Vec3,
    pub rot: f32,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug, Serialize, Deserialize)]
pub enum BlockKind {
    #[default]
    Air,
    Stone,
    Dirt,
    Grass,
    Plank,
    Bedrock,
    Water,
    Sand,
    Wood,
    Leaf,
    Snow,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GameEntityKind {
    Ferris,
}

#[derive(Component)]
struct ComputeChunk(Task<(Chunk, IVec3)>, u32);

#[derive(Component)]
struct ComputeChunkMesh(Task<Option<ChunkMesh>>, u32);

fn handle_chunk_gen(
    mut commands: Commands,
    game_info: Res<GameInfo>,
    game_settings: Res<GameSettings>,
    player: Single<&Transform, With<Player>>,
) {
    let pt = player.translation;
    let thread_pool = AsyncComputeTaskPool::get();
    let render_distance = game_settings.render_distance;

    for chunk_z in
        (pt.z as i32 / CHUNK_SIZE - render_distance)..(pt.z as i32 / CHUNK_SIZE + render_distance)
    {
        for chunk_x in (pt.x as i32 / CHUNK_SIZE - render_distance)
            ..(pt.x as i32 / CHUNK_SIZE + render_distance)
        {
            let pos = ivec3(chunk_x, 0, chunk_z);

            let Ok(chunks_guard) = game_info.chunks.read() else {
                continue;
            };
            let Ok(loading_chunks_guard) = game_info.loading_chunks.read() else {
                continue;
            };

            if chunks_guard.contains_key(&pos) || loading_chunks_guard.contains(&pos) {
                continue;
            }
            drop(chunks_guard);
            drop(loading_chunks_guard);

            game_info.loading_chunks.write().unwrap().insert(pos);

            let seed = game_info.seed;
            let chunks = game_info.chunks.clone();
            let saved_chunks = game_info.saved_chunks.clone();
            let task = thread_pool.spawn(async move {
                let mut chunk = Chunk::new(pos);

                for rela_z in 0..CHUNK_SIZE {
                    for rela_x in 0..CHUNK_SIZE {
                        let pos = vec2(
                            (rela_x + pos.x * CHUNK_SIZE) as f32,
                            (rela_z + pos.z * CHUNK_SIZE) as f32,
                        );
                        let (max_y, biome) = terrain_noise(pos, seed);

                        for y in 0..CHUNK_HEIGHT {
                            chunk.blocks[vec3_to_index(ivec3(rela_x, y, rela_z))] =
                                generate_block_at(ivec3(pos.x as i32, y, pos.y as i32), seed);

                            if y == max_y
                                && max_y > SEA_LEVEL
                                && biome < 0.4
                                && ferris_noise(pos, seed) > 0.85
                            {
                                chunk.entities.push((
                                    Entity::PLACEHOLDER,
                                    GameEntity {
                                        kind: GameEntityKind::Ferris,
                                        pos: vec3(pos.x, y as f32, pos.y),
                                        rot: rand::random_range(0..360) as f32,
                                    },
                                ));
                            }
                        }

                        let tree_probabilty = tree_noise(pos, seed);

                        if tree_probabilty > 0.85 && max_y < 90 && max_y > SEA_LEVEL + 2 {
                            for (y, tree_layer) in TREE_OBJECT.iter().enumerate() {
                                for (z, tree_row) in tree_layer.iter().enumerate() {
                                    for (x, block) in tree_row.iter().enumerate() {
                                        let mut pos = ivec3(3 + x as i32, y as i32, 3 + z as i32);
                                        let (local_max_y, _) = terrain_noise(
                                            (chunk.pos * CHUNK_SIZE + pos).as_vec3().xz(),
                                            seed,
                                        );

                                        pos.y += local_max_y;

                                        if (0..CHUNK_SIZE).contains(&pos.x)
                                            && (0..CHUNK_HEIGHT).contains(&pos.y)
                                            && (0..CHUNK_SIZE).contains(&pos.z)
                                        {
                                            chunk.blocks[vec3_to_index(pos)] = *block;
                                        } else if let Some(relative_chunk_key) =
                                            chunk.get_relative_chunk(pos)
                                            && let Some(target_chunk) =
                                                chunks.write().unwrap().get_mut(&relative_chunk_key)
                                        {
                                            let block_index = vec3_to_index(
                                                pos - relative_chunk_key * CHUNK_SIZE,
                                            );
                                            if block_index < target_chunk.blocks.len() {
                                                target_chunk.blocks[block_index] = *block;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(saved_chunk) = saved_chunks.read().unwrap().get(&pos) {
                    for (pos, block) in &saved_chunk.blocks {
                        chunk.blocks[vec3_to_index(*pos)] = *block;
                    }
                    chunk.entities = saved_chunk.entities.clone();
                }
                (chunk, pos)
            });
            commands.spawn(ComputeChunk(
                task,
                pos.distance_squared(pt.as_ivec3().with_y(0) / CHUNK_SIZE) as u32,
            ));
        }
    }
}

fn handle_mesh_gen(
    mut commands: Commands,
    player: Single<&Transform, With<Player>>,
    game_info: Res<GameInfo>,
    chunks_query: Query<(Entity, &Transform), Added<ChunkMarker>>,
) {
    let thread_pool = AsyncComputeTaskPool::get();

    for (entity, chunk_transform) in chunks_query.iter() {
        let chunk_coords = chunk_transform.translation.as_ivec3() / CHUNK_SIZE;

        let chunks_for_task = game_info.chunks.clone();
        let seed = game_info.seed;

        let task = thread_pool.spawn(async move {
            let chunks_map_guard = chunks_for_task.read().unwrap();

            let chunk_data_option = chunks_map_guard.get(&chunk_coords);

            if let Some(chunk_data) = chunk_data_option {
                build_chunk_mesh(chunk_data, &chunks_map_guard, seed)
            } else {
                None
            }
        });

        commands.entity(entity).try_insert(ComputeChunkMesh(
            task,
            (chunk_transform.translation.as_ivec3() / CHUNK_SIZE)
                .distance_squared(player.translation.as_ivec3().with_y(0) / CHUNK_SIZE)
                as u32,
        ));
    }
}

fn handle_chunk_despawn(
    mut commands: Commands,
    game_info: Res<GameInfo>,
    game_settings: Res<GameSettings>,
    chunks_query: Query<(Entity, &Transform), With<ChunkMarker>>,
    player: Single<&Transform, With<Player>>,
) {
    let pt = player.translation;
    let render_distance = game_settings.render_distance;

    let mut chunks_to_check: Vec<(IVec3, Entity)> = Vec::new();
    for (entity, transform) in chunks_query.iter() {
        let chunk_key = transform.translation.as_ivec3() / CHUNK_SIZE;
        chunks_to_check.push((chunk_key, entity));
    }

    if chunks_to_check.is_empty() {
        return;
    }

    let mut chunks_guard = game_info.chunks.write().unwrap();
    let mut loading_chunks_guard = game_info.loading_chunks.write().unwrap();
    for (chunk_key, entity) in chunks_to_check {
        if (chunk_key.x + render_distance < pt.x as i32 / CHUNK_SIZE)
            || (chunk_key.x - render_distance > pt.x as i32 / CHUNK_SIZE)
            || (chunk_key.z + render_distance < pt.z as i32 / CHUNK_SIZE)
            || (chunk_key.z - render_distance > pt.z as i32 / CHUNK_SIZE)
        {
            let chunk_entities = &chunks_guard.get(&chunk_key).map(|x| &x.entities);
            if let Some(chunk_entities) = chunk_entities {
                for (entity, _) in chunk_entities.iter() {
                    if *entity != Entity::PLACEHOLDER {
                        commands.entity(*entity).try_despawn();
                    }
                }
            }
            commands.entity(entity).try_despawn();

            chunks_guard.remove(&chunk_key);
            loading_chunks_guard.remove(&chunk_key);
        }
    }
}

fn process_tasks(
    mut commands: Commands,
    mut mesh_tasks: Query<(Entity, &mut ComputeChunkMesh)>,
    mut spawn_tasks: Query<(Entity, &mut ComputeChunk)>,
    mut meshes: ResMut<Assets<Mesh>>,
    game_info: Res<GameInfo>,
) {
    // SPAWNING CHUNKS
    let mut processed_this_frame = 0;
    let mut tasks = spawn_tasks.iter_mut().collect::<Vec<_>>();

    tasks.sort_by(|(_, first), (_, second)| first.1.cmp(&second.1)); // should matter but i dont think it does.

    for (entity, mut compute_task) in tasks {
        if processed_this_frame >= 15 {
            break;
        }
        if let Some((mut chunk, pos)) = future::block_on(future::poll_once(&mut compute_task.0)) {
            let mut saved_chunks_guard = game_info.saved_chunks.write().unwrap();
            if let Some(saved_chunk) = saved_chunks_guard.get_mut(&pos) {
                if saved_chunk.entities != chunk.entities {
                    saved_chunk.entities = chunk.entities.clone();
                }
            } else {
                saved_chunks_guard.insert(
                    pos,
                    SavedChunk {
                        pos,
                        entities: chunk.entities.clone(),
                        ..default()
                    },
                );
            }
            drop(saved_chunks_guard);

            for (e, game_entity) in chunk.entities.iter_mut() {
                *e = commands
                    .spawn((
                        *game_entity,
                        SceneRoot(game_info.models[game_entity.kind as usize].clone()),
                        Transform::from_translation(game_entity.pos + vec3(0.5, 0.0, 0.5))
                            .with_scale(Vec3::splat(2.0))
                            .with_rotation(Quat::from_rotation_y(game_entity.rot)),
                    ))
                    .id();
            }
            commands
                .entity(entity)
                .try_insert((
                    ChunkMarker,
                    Aabb::from_min_max(
                        vec3(0.0, 0.0, 0.0),
                        vec3(CHUNK_SIZE as f32, CHUNK_HEIGHT as f32, CHUNK_SIZE as f32),
                    ),
                    Transform::from_translation((pos * CHUNK_SIZE).as_vec3()),
                ))
                .try_remove::<ComputeChunk>();

            game_info.chunks.write().unwrap().insert(pos, chunk);
            game_info.loading_chunks.write().unwrap().remove(&pos);

            processed_this_frame += 1;
        }
    }

    // GENERATING MESHES
    let mut processed_this_frame = 0;

    let mut tasks = mesh_tasks.iter_mut().collect::<Vec<_>>();
    tasks.sort_by(|(_, first), (_, second)| first.1.cmp(&second.1));
    for (entity, mut compute_task) in tasks {
        if processed_this_frame >= 15 {
            break;
        }

        if let Some(result) = future::block_on(future::poll_once(&mut compute_task.0)) {
            commands.entity(entity).try_remove::<ComputeChunkMesh>();

            if let Some(mesh_data) = result {
                let mut bevy_mesh = Mesh::new(
                    PrimitiveTopology::TriangleList,
                    RenderAssetUsages::RENDER_WORLD,
                );
                let mut positions = Vec::new();
                let mut normals = Vec::new();

                for &vertex in mesh_data.vertices.iter() {
                    let (pos, normal, _block_type) = get_vertex_u32(vertex);
                    positions.push(pos);
                    normals.push(normal);
                }

                bevy_mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
                bevy_mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
                bevy_mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, mesh_data.uvs);
                bevy_mesh.insert_indices(Indices::U32(mesh_data.indices));

                let mesh_handle = meshes.add(bevy_mesh);

                commands.entity(entity).try_insert((
                    Mesh3d(mesh_handle),
                    MeshMaterial3d(game_info.materials[0].clone()),
                    Visibility::Visible,
                ));
            } else {
                error!("Error building chunk mesh for entity {:?}", entity);
            }
            processed_this_frame += 1;
        }
    }
}
