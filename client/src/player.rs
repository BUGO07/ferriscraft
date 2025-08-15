use crate::{
    CHUNK_HEIGHT, CHUNK_SIZE, GameInfo, PausableSystems,
    render_pipeline::PostProcessSettings,
    ui::GameState,
    utils::{aabb_collision, ray_cast, vec3_to_index},
    world::{
        ChunkMarker,
        utils::{NoiseFunctions, place_block, terrain_noise},
    },
};
use bevy::{
    core_pipeline::{
        Skybox, bloom::Bloom, experimental::taa::TemporalAntiAliasing, tonemapping::Tonemapping,
    },
    input::mouse::MouseMotion,
    pbr::ScreenSpaceAmbientOcclusion,
    prelude::*,
    render::primitives::Aabb,
    window::{CursorGrabMode, PrimaryWindow},
};
use bevy_renet::renet::RenetClient;
use ferriscraft::{Block, ClientPacket};

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (camera_movement, handle_interactions).run_if(
                not(in_state(GameState::Menu)).and(|game_info: Res<GameInfo>| !game_info.paused),
            ),
        )
        .add_systems(
            FixedUpdate,
            player_movement
                .run_if(
                    // only run if chunks have been loaded
                    |game_info: Option<Res<GameInfo>>, mut is_loaded: Local<bool>| {
                        if !*is_loaded && let Some(game_info) = game_info {
                            *is_loaded = game_info.chunks.read().unwrap().len()
                                == ((game_info.settings.render_distance * 2)
                                    * (game_info.settings.render_distance * 2))
                                    as usize;
                        }
                        *is_loaded
                    },
                )
                .run_if(not(in_state(GameState::Menu)))
                .in_set(PausableSystems),
        );
    }
}

#[derive(Component, Default, Clone, Copy)]
pub struct Player {
    pub velocity: Vec3,
}

#[derive(Component)]
pub struct OnlinePlayer(pub String);

fn handle_interactions(
    mut commands: Commands,
    mut gizmos: Gizmos,
    client: Option<ResMut<RenetClient>>,
    game_info: Res<GameInfo>,
    player: Single<&Transform, With<Player>>,
    camera: Single<&GlobalTransform, With<Camera3d>>,
    chunks: Query<(Entity, &Transform), (With<ChunkMarker>, Without<OnlinePlayer>)>,
    online_players: Query<&Transform, (With<OnlinePlayer>, Without<ChunkMarker>)>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    if let Some(hit) = ray_cast(
        &game_info,
        camera.translation(),
        (camera.rotation() * Vec3::NEG_Z).normalize_or_zero(),
        5.0,
    ) {
        let hit_global_position = hit.global_position;
        let mut local_pos = hit.local_pos;
        let mut chunk_pos = hit.chunk_pos;

        gizmos.cuboid(
            Transform::from_translation(hit_global_position.as_vec3() + Vec3::splat(0.5)),
            Color::srgb(1.0, 0.0, 0.0),
        );

        if mouse.just_pressed(MouseButton::Left) {
            if let Some(chunk) = game_info.chunks.write().unwrap().get_mut(&chunk_pos) {
                let mut saved_chunks = if let Some(saved_chunks) = &game_info.saved_chunks {
                    Some(&mut *saved_chunks.write().unwrap())
                } else {
                    None
                };
                place_block(
                    chunk,
                    local_pos,
                    Block::AIR,
                    &mut saved_chunks,
                    client,
                    Some((&mut commands, chunks.iter().collect())),
                );
            }
        } else if mouse.just_pressed(MouseButton::Right) {
            local_pos += hit.normal.as_vec3().as_ivec3();

            if local_pos.y >= 0 && local_pos.y < CHUNK_HEIGHT - 1 {
                if local_pos.x < 0 {
                    local_pos.x += CHUNK_SIZE;
                    chunk_pos.x -= 1;
                } else if local_pos.x >= CHUNK_SIZE {
                    local_pos.x -= CHUNK_SIZE;
                    chunk_pos.x += 1;
                }

                if local_pos.z < 0 {
                    local_pos.z += CHUNK_SIZE;
                    chunk_pos.z -= 1;
                } else if local_pos.z >= CHUNK_SIZE {
                    local_pos.z -= CHUNK_SIZE;
                    chunk_pos.z += 1;
                }

                if aabb_collision(
                    player.translation,
                    vec3(0.25, 1.8, 0.25),
                    hit_global_position.as_vec3() + hit.normal.as_vec3(),
                    vec3(1.25, 1.0, 1.25),
                ) {
                    return;
                }

                for online_player in online_players.iter() {
                    if aabb_collision(
                        online_player.translation,
                        vec3(0.25, 1.8, 0.25),
                        hit_global_position.as_vec3() + hit.normal.as_vec3(),
                        vec3(1.25, 1.0, 1.25),
                    ) {
                        return;
                    }
                }

                if let Some(chunk) = game_info.chunks.write().unwrap().get_mut(&chunk_pos) {
                    if chunk.blocks[vec3_to_index(local_pos)] == Block::AIR {
                        let mut saved_chunks = if let Some(saved_chunks) = &game_info.saved_chunks {
                            Some(&mut *saved_chunks.write().unwrap())
                        } else {
                            None
                        };
                        place_block(
                            chunk,
                            local_pos,
                            Block {
                                kind: game_info.current_block,
                                direction: if game_info.current_block.can_rotate() {
                                    hit.normal
                                } else {
                                    Default::default()
                                },
                            },
                            &mut saved_chunks,
                            client,
                            Some((&mut commands, chunks.iter().collect())),
                        );
                    }
                } else {
                    warn!("placing in a chunk that doesn't exist {:?}", chunk_pos);
                }
            }
        }
    }
}

fn camera_movement(
    mut camera: Single<&mut Transform, (With<Camera3d>, Without<Player>)>,
    mut player: Single<&mut Transform, (With<Player>, Without<Camera3d>)>,
    mut mouse: EventReader<MouseMotion>,
    game_info: Res<GameInfo>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    for ev in mouse.read() {
        let (_, mut pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
        let (mut yaw, _, _) = player.rotation.to_euler(EulerRot::YXZ);

        if window.cursor_options.grab_mode != CursorGrabMode::None {
            let window_scale = window.height().min(window.width());
            pitch -= (game_info.settings.sensitivity * ev.delta.y * window_scale / 10_000.0)
                .to_radians();
            yaw -= (game_info.settings.sensitivity * ev.delta.x * window_scale / 10_000.0)
                .to_radians();
        }

        pitch = pitch.clamp(-1.54, 1.54);

        camera.rotation = Quat::from_axis_angle(Vec3::X, pitch);
        player.rotation = Quat::from_axis_angle(Vec3::Y, yaw);
    }
}

fn player_movement(
    client: Option<ResMut<RenetClient>>,
    player: Single<(&mut Transform, &mut Player)>,
    keyboard: Res<ButtonInput<KeyCode>>,
    game_info: Res<GameInfo>,
    time: Res<Time>,
) {
    let (mut transform, mut player) = player.into_inner();

    let delta = time.delta_secs();

    let mut move_dir = Vec3::ZERO;
    let mut sprint_multiplier = 1.0;

    let sneaking = keyboard.pressed(KeyCode::ShiftLeft);

    if !game_info.paused {
        let local_z = transform.local_z();
        let forward = -Vec3::new(local_z.x, 0.0, local_z.z).normalize_or_zero();
        let right = Vec3::new(local_z.z, 0.0, -local_z.x).normalize_or_zero();

        if keyboard.pressed(KeyCode::KeyW) {
            if !sneaking && keyboard.pressed(KeyCode::ControlLeft) {
                sprint_multiplier = 1.3;
            }
            move_dir += forward;
        }
        if keyboard.pressed(KeyCode::KeyS) {
            move_dir -= forward;
        }
        if keyboard.pressed(KeyCode::KeyA) {
            move_dir -= right;
        }
        if keyboard.pressed(KeyCode::KeyD) {
            move_dir += right;
        }

        move_dir = move_dir.normalize_or_zero();
    }

    let mut target_velocity = vec3(
        move_dir.x * game_info.settings.movement_speed * sprint_multiplier,
        0.0,
        move_dir.z * game_info.settings.movement_speed * sprint_multiplier,
    );

    if sneaking {
        target_velocity *= 0.3;

        // if ray_cast(&game_info, transform.translation, -Vec3::Y, 0.2).is_none() {
        // TODO
        // }
    }

    let movement_collision_offsets = &[
        vec3(0.25, 0.0, 0.25),
        vec3(-0.25, 0.0, 0.25),
        vec3(0.25, 0.0, -0.25),
        vec3(-0.25, 0.0, -0.25),
        vec3(0.25, 1.0, 0.25),
        vec3(-0.25, 1.0, 0.25),
        vec3(0.25, 1.0, -0.25),
        vec3(-0.25, 1.0, -0.25),
        vec3(0.0, 0.0, 0.0),
        vec3(0.0, 1.0, 0.0),
    ];

    if target_velocity.x != 0.0 {
        let move_x = Vec3::new(target_velocity.x * delta, 0.0, 0.0);
        let dir_x = move_x.normalize_or_zero();
        let distance_x = move_x.length() + 0.05;

        for offset in movement_collision_offsets {
            let origin = transform.translation + *offset + Vec3::Y * 0.01;
            if let Some(hit) = ray_cast(&game_info, origin, dir_x, distance_x)
                && hit.normal.as_vec3().dot(dir_x) < -0.1
            {
                target_velocity.x = 0.0;
                break;
            }
        }
    }

    if target_velocity.z != 0.0 {
        let move_z = Vec3::new(0.0, 0.0, target_velocity.z * delta);
        let dir_z = move_z.normalize_or_zero();
        let distance_z = move_z.length() + 0.05;

        for offset in movement_collision_offsets {
            let origin = transform.translation + *offset + Vec3::Y * 0.01;
            if let Some(hit) = ray_cast(&game_info, origin, dir_z, distance_z)
                && hit.normal.as_vec3().dot(dir_z) < -0.1
            {
                target_velocity.z = 0.0;
                break;
            }
        }
    }

    player.velocity.x = target_velocity.x;
    player.velocity.z = target_velocity.z;

    let mut grounded = false;
    let mut closest_ground_distance = f32::MAX;

    let grounded_offsets = &[
        vec3(0.25, 0.1, 0.25),
        vec3(-0.25, 0.1, 0.25),
        vec3(0.25, 0.1, -0.25),
        vec3(-0.25, 0.1, -0.25),
        vec3(0.0, 0.1, 0.0),
    ];

    for offset in grounded_offsets {
        let origin = transform.translation + *offset;
        let fall_distance = player.velocity.y.abs() * delta + 0.1;

        if let Some(hit) = ray_cast(&game_info, origin, -Vec3::Y, fall_distance) {
            grounded = true;

            if hit.distance < closest_ground_distance {
                closest_ground_distance = hit.distance;
            }
        }
    }

    if grounded {
        if !game_info.paused && keyboard.pressed(KeyCode::Space) {
            let mut head_blocked = false;
            for offset in grounded_offsets {
                let origin = transform.translation + Vec3::Y * 1.8 + *offset;
                if ray_cast(&game_info, origin, Vec3::Y, 0.3).is_some() {
                    head_blocked = true;
                    break;
                }
            }

            player.velocity.y = if head_blocked {
                game_info.settings.jump_force / 4.0
            } else {
                game_info.settings.jump_force
            };
        } else {
            player.velocity.y = 0.0;
        }

        if player.velocity.y <= 0.0
            && closest_ground_distance > 0.0
            && closest_ground_distance < 0.1
        {
            transform.translation.y -= closest_ground_distance - 0.1;
        }
    } else {
        player.velocity.y += game_info.settings.gravity * delta;
        player.velocity.y = player.velocity.y.max(-78.4);
    }

    transform.translation += player.velocity * delta;

    if player.velocity.length() > 0.0 {
        ClientPacket::Move(transform.translation).send(client);
    }
}

pub fn player_bundle(
    player_pos: Vec3,
    player_velocity: Vec3,
    player_yaw: f32,
    noises: &NoiseFunctions,
) -> impl Bundle {
    (
        Transform::from_translation(if player_pos == Vec3::INFINITY {
            vec3(0.0, 1.0 + terrain_noise(Vec2::ZERO, noises).0 as f32, 0.0)
        } else {
            player_pos
        })
        .with_rotation(Quat::from_rotation_y(player_yaw)),
        Aabb::from_min_max(vec3(-0.25, 0.0, -0.25), vec3(0.25, 1.8, 0.25)),
        Player {
            velocity: player_velocity,
        },
        Visibility::Visible,
    )
}

pub fn camera_bundle(skybox: Handle<Image>, player: Entity, pitch: f32) -> impl Bundle {
    (
        Camera3d::default(),
        Camera {
            hdr: true,
            ..default()
        },
        Msaa::Off,
        TemporalAntiAliasing::default(),
        PostProcessSettings::default(),
        Skybox {
            image: skybox,
            brightness: 1000.0,
            ..default()
        },
        Bloom::NATURAL,
        Tonemapping::TonyMcMapface,
        ScreenSpaceAmbientOcclusion::default(),
        Transform::from_xyz(0.0, 1.62, -0.05).with_rotation(Quat::from_rotation_x(pitch)), // minecraft way
        ChildOf(player),
    )
}
