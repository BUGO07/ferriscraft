use bevy::{
    asset::RenderAssetUsages,
    prelude::*,
    render::{
        mesh::{Indices, PrimitiveTopology},
        primitives::Aabb,
    },
    tasks::{AsyncComputeTaskPool, futures_lite::future},
    window::PrimaryWindow,
};
use bevy_persistent::Persistent;
use bevy_renet::renet::RenetClient;
use ferriscraft::{ClientPacket, GameEntity, GameEntityKind, SEA_LEVEL, SavedChunk, SavedWorld};
use rayon::slice::ParallelSliceMut;

use crate::{
    CHUNK_HEIGHT, CHUNK_SIZE, GameInfo, GameSettings,
    player::Player,
    utils::{TREE_OBJECT, noise, vec3_to_index},
    world::{
        Chunk, ChunkMarker, ComputeChunk, ComputeChunkMesh,
        mesher::ChunkMesh,
        utils::{generate_block_at, terrain_noise},
    },
};

pub fn autosave_and_exit(
    mut app_exit: EventWriter<AppExit>,
    mut last_save: Local<f32>,
    persistent_world: Option<ResMut<Persistent<SavedWorld>>>,
    client: Option<ResMut<RenetClient>>,
    window: Query<&Window, With<PrimaryWindow>>,
    player: Query<(&Transform, &Player)>,
    camera: Query<&Transform, With<Camera3d>>,
    game_settings: Res<GameSettings>,
    game_info: Option<Res<GameInfo>>,
    time: Res<Time>,
) {
    if window.is_empty() {
        info!("saving and exiting");
        save_game(
            persistent_world,
            player,
            camera.single().ok(),
            game_info.as_deref(),
        );
        if let Some(mut client) = client {
            client.disconnect();
        }
        app_exit.write(AppExit::Success);
        return;
    }

    let elapsed = time.elapsed_secs_wrapped();

    // 10 minute autosave
    if game_settings.autosave && elapsed > *last_save + 600.0 {
        save_game(
            persistent_world,
            player,
            camera.single().ok(),
            game_info.as_deref(),
        );
        *last_save = elapsed;
    }

    if elapsed < *last_save {
        *last_save = elapsed;
    }
}

pub fn save_game(
    persistent_world: Option<ResMut<Persistent<SavedWorld>>>,
    player: Query<(&Transform, &Player)>,
    camera: Option<&Transform>,
    game_info: Option<&GameInfo>,
) {
    if let Some(mut persistent_world) = persistent_world
        && let Some(game_info) = game_info
    {
        persistent_world
            .update(|sc| {
                if let Ok(player) = player.single()
                    && let Some(camera) = camera
                {
                    let (_, pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
                    let (yaw, _, _) = player.0.rotation.to_euler(EulerRot::YXZ);
                    sc.1.insert(
                        game_info.player_name.clone(),
                        (player.0.translation, player.1.velocity, yaw, pitch),
                    );
                }
                if let Some(saved_chunks) = &game_info.saved_chunks {
                    sc.2 = saved_chunks.read().unwrap().clone();
                }
            })
            .unwrap();
    }
}

pub fn handle_chunk_gen(
    mut commands: Commands,
    game_info: Res<GameInfo>,
    game_settings: Res<GameSettings>,
    player: Single<&Transform, With<Player>>,
    client: Option<ResMut<RenetClient>>,
) {
    let pt = player.translation;
    let thread_pool = AsyncComputeTaskPool::get();
    let render_distance = game_settings.render_distance;
    let noises = game_info.noises;

    let mut chunks_to_load = Vec::new();

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

            chunks_to_load.push(pos);

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
                        let (max_y, biome) = terrain_noise(pos, &noises);

                        for y in 0..CHUNK_HEIGHT {
                            chunk.blocks[vec3_to_index(ivec3(rela_x, y, rela_z))] =
                                generate_block_at(ivec3(pos.x as i32, y, pos.y as i32), max_y);

                            if y == max_y
                                && max_y > SEA_LEVEL
                                && biome < 0.4
                                && noise(noises.ferris, pos) > 0.85
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

                        let tree_probabilty = noise(noises.tree, pos);

                        // TODO: clean up
                        if tree_probabilty > 0.85 && max_y < 90 && max_y > SEA_LEVEL + 2 {
                            for (y, tree_layer) in TREE_OBJECT.iter().enumerate() {
                                for (z, tree_row) in tree_layer.iter().enumerate() {
                                    for (x, &block) in tree_row.iter().enumerate() {
                                        let mut pos = ivec3(3 + x as i32, y as i32, 3 + z as i32);
                                        let (local_max_y, _) = terrain_noise(
                                            (chunk.pos * CHUNK_SIZE + pos).as_vec3().xz(),
                                            &noises,
                                        );

                                        pos.y += local_max_y;

                                        if (0..CHUNK_SIZE).contains(&pos.x)
                                            && (0..CHUNK_HEIGHT).contains(&pos.y)
                                            && (0..CHUNK_SIZE).contains(&pos.z)
                                        {
                                            chunk.blocks[vec3_to_index(pos)] = block;
                                        } else if let Some(relative_chunk) =
                                            chunk.get_relative_chunk(pos)
                                            && let Some(target) =
                                                chunks.write().unwrap().get_mut(&relative_chunk)
                                        {
                                            let block_index =
                                                vec3_to_index(pos - relative_chunk * CHUNK_SIZE);
                                            if block_index < target.blocks.len() {
                                                target.blocks[block_index] = block;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(saved_chunks) = &saved_chunks
                    && let Some(saved_chunk) = saved_chunks.read().unwrap().get(&pos)
                {
                    for (&pos, &block) in &saved_chunk.blocks {
                        chunk.blocks[vec3_to_index(pos)] = block;
                    }
                    chunk.entities = saved_chunk.entities.clone();
                }
                chunk
            });
            commands.spawn(ComputeChunk(task, pos));
        }
    }
    if !chunks_to_load.is_empty() {
        ClientPacket::LoadChunks(chunks_to_load).send(client);
    }
}

pub fn handle_mesh_gen(
    mut commands: Commands,
    game_info: Res<GameInfo>,
    query: Query<(Entity, &Transform), Added<ChunkMarker>>,
) {
    let thread_pool = AsyncComputeTaskPool::get();

    for (entity, transform) in query {
        let pos = transform.translation.as_ivec3() / CHUNK_SIZE;

        let chunks = game_info.chunks.clone();
        let noises = game_info.noises;

        let task = thread_pool.spawn(async move {
            let guard = chunks.read().unwrap();
            #[cfg(feature = "profile")]
            let instant = std::time::Instant::now();
            let mesh = ChunkMesh::default().build(guard.get(&pos)?, &guard, &noises);
            #[cfg(feature = "profile")]
            println!("Generated chunk in {:?}", instant.elapsed());
            mesh
        });

        commands
            .entity(entity)
            .try_insert(ComputeChunkMesh(task, pos));
    }
}

pub fn handle_chunk_despawn(
    mut commands: Commands,
    game_info: Res<GameInfo>,
    game_settings: Res<GameSettings>,
    query: Query<
        (Entity, &Transform),
        Or<(
            With<ChunkMarker>,
            With<ComputeChunkMesh>,
            With<ComputeChunk>,
        )>,
    >,
    player: Single<&Transform, With<Player>>,
) {
    let pt = player.translation;
    let render_distance = game_settings.render_distance;

    let mut chunks = game_info.chunks.write().unwrap();
    let mut loading_chunks = game_info.loading_chunks.write().unwrap();

    for (entity, transform) in query {
        let pos = transform.translation.as_ivec3() / CHUNK_SIZE;

        if (pos.x + render_distance < pt.x as i32 / CHUNK_SIZE)
            || (pos.x - render_distance > pt.x as i32 / CHUNK_SIZE)
            || (pos.z + render_distance < pt.z as i32 / CHUNK_SIZE)
            || (pos.z - render_distance > pt.z as i32 / CHUNK_SIZE)
        {
            {
                if let Some(chunk_entities) = chunks.get(&pos) {
                    for (entity, _) in &chunk_entities.entities {
                        if *entity != Entity::PLACEHOLDER {
                            commands.entity(*entity).try_despawn();
                        }
                    }
                }
            }
            commands.entity(entity).try_despawn();

            chunks.remove(&pos);
            loading_chunks.remove(&pos);
        }
    }
}

pub fn process_tasks(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    player: Single<&Transform, With<Player>>,
    mesh_tasks: Query<(Entity, &mut ComputeChunkMesh)>,
    spawn_tasks: Query<(Entity, &mut ComputeChunk)>,
    game_info: Res<GameInfo>,
) {
    // GENERATING CHUNKS
    let pt = player.translation.as_ivec3().with_y(0) / CHUNK_SIZE;

    let mut tasks = spawn_tasks.into_iter().collect::<Vec<_>>();
    tasks.par_sort_by_cached_key(|(_, x)| x.1.distance_squared(pt));

    let mut chunks = game_info.chunks.write().unwrap();
    let mut saved_chunks = game_info
        .saved_chunks
        .as_ref()
        .map(|saved_chunks| saved_chunks.write().unwrap());
    let mut loading_chunks = game_info.loading_chunks.write().unwrap();

    let mut processed_this_frame = 0;
    for (entity, mut compute_task) in tasks {
        if processed_this_frame >= 15 {
            break;
        }
        if let Some(mut chunk) = future::block_on(future::poll_once(&mut compute_task.0)) {
            if let Some(saved_chunks) = &mut saved_chunks {
                saved_chunks
                    .entry(chunk.pos)
                    .and_modify(|c| {
                        if c.entities != chunk.entities {
                            c.entities = chunk.entities.clone();
                        }
                    })
                    .or_insert(SavedChunk {
                        entities: chunk.entities.clone(),
                        ..default()
                    });
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

            loading_chunks.remove(&chunk.pos);
            chunks.insert(chunk.pos, chunk);

            processed_this_frame += 1;
        }
    }

    // GENERATING MESHES

    let mut tasks = mesh_tasks.into_iter().collect::<Vec<_>>();
    tasks.par_sort_by_cached_key(|(_, x)| x.1.distance_squared(pt));

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
