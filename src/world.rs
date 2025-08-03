use std::{
    collections::{HashMap, hash_map::Entry},
    path::Path,
};

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
    mesher::ChunkMesh,
    player::Player,
    utils::{
        Direction, TREE_OBJECT, ferris_noise, generate_block_at, terrain_noise, tree_noise,
        vec3_to_index,
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
                .default(SavedWorld(
                    rand::random(),
                    (Vec3::INFINITY, 0.0, 0.0),
                    HashMap::new(),
                ))
                .build()
                .expect("World save couldn't be read, please make a backup of saves/world.ferris and remove it from the saves folder."),
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
    pub entities: Vec<(Entity, GameEntity)>,
    pub blocks: HashMap<IVec3, Block>, // placed/broken blocks
}

#[derive(Resource, Clone, Default, Serialize, Deserialize)]
pub struct SavedWorld(
    pub u32,
    pub (Vec3, f32, f32),
    pub HashMap<IVec3, SavedChunk>,
);

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
struct ComputeChunk(Task<Chunk>, u32);

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

            if let Ok(guard) = game_info.chunks.read() {
                if guard.contains_key(&pos) {
                    continue;
                }
            } else {
                continue;
            };

            if let Ok(guard) = game_info.loading_chunks.read() {
                if guard.contains(&pos) {
                    continue;
                }
            } else {
                continue;
            };

            {
                game_info.loading_chunks.write().unwrap().insert(pos);
            }

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

                        // TODO: clean up
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
                                        } else if let Some(relative_chunk) =
                                            chunk.get_relative_chunk(pos)
                                            && let Some(target) =
                                                chunks.write().unwrap().get_mut(&relative_chunk)
                                        {
                                            let block_index =
                                                vec3_to_index(pos - relative_chunk * CHUNK_SIZE);
                                            if block_index < target.blocks.len() {
                                                target.blocks[block_index] = *block;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(saved_chunk) = saved_chunks.read().unwrap().get(&pos) {
                    for (&pos, &block) in &saved_chunk.blocks {
                        chunk.blocks[vec3_to_index(pos)] = block;
                    }
                    chunk.entities = saved_chunk.entities.clone();
                }
                chunk
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
    query: Query<(Entity, &Transform), Added<ChunkMarker>>,
) {
    let thread_pool = AsyncComputeTaskPool::get();

    for (entity, transform) in query {
        let pos = transform.translation.as_ivec3() / CHUNK_SIZE;

        let chunks = game_info.chunks.clone();
        let seed = game_info.seed;

        let task = thread_pool.spawn(async move {
            let guard = chunks.read().unwrap();
            ChunkMesh::default().build(guard.get(&pos)?, &guard, seed)
        });

        commands.entity(entity).try_insert(ComputeChunkMesh(
            task,
            (transform.translation.as_ivec3() / CHUNK_SIZE)
                .distance_squared(player.translation.as_ivec3().with_y(0) / CHUNK_SIZE)
                as u32,
        ));
    }
}

fn handle_chunk_despawn(
    mut commands: Commands,
    game_info: Res<GameInfo>,
    game_settings: Res<GameSettings>,
    query: Query<(Entity, &Transform), With<ChunkMarker>>,
    player: Single<&Transform, With<Player>>,
) {
    let pt = player.translation;
    let render_distance = game_settings.render_distance;

    for (entity, transform) in query {
        let pos = transform.translation.as_ivec3() / CHUNK_SIZE;

        if (pos.x + render_distance < pt.x as i32 / CHUNK_SIZE)
            || (pos.x - render_distance > pt.x as i32 / CHUNK_SIZE)
            || (pos.z + render_distance < pt.z as i32 / CHUNK_SIZE)
            || (pos.z - render_distance > pt.z as i32 / CHUNK_SIZE)
        {
            {
                if let Some(chunk_entities) = game_info
                    .chunks
                    .read()
                    .unwrap()
                    .get(&pos)
                    .map(|x| &x.entities)
                {
                    for &(entity, _) in chunk_entities {
                        if entity != Entity::PLACEHOLDER {
                            commands.entity(entity).try_despawn();
                        }
                    }
                }
            }
            commands.entity(entity).try_despawn();

            game_info.chunks.write().unwrap().remove(&pos);
            game_info.loading_chunks.write().unwrap().remove(&pos);
        }
    }
}

fn process_tasks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mesh_tasks: Query<(Entity, &mut ComputeChunkMesh)>,
    spawn_tasks: Query<(Entity, &mut ComputeChunk)>,
    game_info: Res<GameInfo>,
) {
    // GENERATING CHUNKS

    let mut tasks = spawn_tasks.into_iter().collect::<Vec<_>>();
    tasks.sort_by(|(_, first), (_, second)| first.1.cmp(&second.1)); // should matter but i dont think it does.

    let mut processed_this_frame = 0;
    for (entity, mut compute_task) in tasks {
        if processed_this_frame >= 15 {
            break;
        }
        if let Some(mut chunk) = future::block_on(future::poll_once(&mut compute_task.0)) {
            {
                match game_info.saved_chunks.write().unwrap().entry(chunk.pos) {
                    Entry::Vacant(e) => {
                        e.insert(SavedChunk {
                            entities: chunk.entities.clone(),
                            ..default()
                        });
                    }
                    Entry::Occupied(mut e) => {
                        let saved_chunk = e.get_mut();
                        if saved_chunk.entities != chunk.entities {
                            saved_chunk.entities = chunk.entities.clone();
                        }
                    }
                }
            }

            for (e, game_entity) in &mut chunk.entities {
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
                    Transform::from_translation((chunk.pos * CHUNK_SIZE).as_vec3()),
                ))
                .try_remove::<ComputeChunk>();

            game_info.loading_chunks.write().unwrap().remove(&chunk.pos);
            game_info.chunks.write().unwrap().insert(chunk.pos, chunk);

            processed_this_frame += 1;
        }
    }

    // GENERATING MESHES

    let mut tasks = mesh_tasks.into_iter().collect::<Vec<_>>();
    tasks.sort_by(|(_, first), (_, second)| first.1.cmp(&second.1));

    let mut processed_this_frame = 0;
    for (entity, mut compute_task) in tasks {
        if processed_this_frame >= 15 {
            break;
        }

        if let Some(result) = future::block_on(future::poll_once(&mut compute_task.0)) {
            commands.entity(entity).try_remove::<ComputeChunkMesh>();

            if let Some(mesh_data) = result {
                let (positions, normals, uvs): (Vec<_>, Vec<_>, Vec<_>) = mesh_data
                    .vertices
                    .iter()
                    .map(|v| (v.pos, v.normal.as_vec3(), v.uv))
                    .collect();

                commands.entity(entity).try_insert((
                    Mesh3d(
                        meshes.add(
                            Mesh::new(
                                PrimitiveTopology::TriangleList,
                                RenderAssetUsages::RENDER_WORLD,
                            )
                            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
                            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
                            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
                            .with_inserted_indices(Indices::U32(mesh_data.indices)),
                        ),
                    ),
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
