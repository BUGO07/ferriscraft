use crate::{GameInfo, GameSettings, PausableSystems, utils::ray_cast};
use bevy::{
    input::mouse::MouseMotion,
    prelude::*,
    window::{CursorGrabMode, PrimaryWindow},
};

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                player_movement.run_if(
                    // only run if chunks have been loaded
                    |game_info: Res<GameInfo>,
                     game_settings: Res<GameSettings>,
                     mut is_loaded: Local<bool>| {
                        if !*is_loaded {
                            *is_loaded = game_info.chunks.read().unwrap().len()
                                == ((game_settings.render_distance * 2)
                                    * (game_settings.render_distance * 2))
                                    as usize;
                        }
                        *is_loaded
                    },
                ),
                camera_movement,
            )
                .in_set(PausableSystems),
        );
    }
}

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct PlayerCamera;

fn camera_movement(
    mut camera: Single<&mut Transform, (With<PlayerCamera>, Without<Player>)>,
    mut player: Single<&mut Transform, (With<Player>, Without<PlayerCamera>)>,
    mut mouse: EventReader<MouseMotion>,
    settings: Res<GameSettings>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    for ev in mouse.read() {
        let (_, mut pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
        let (mut yaw, _, _) = player.rotation.to_euler(EulerRot::YXZ);

        if window.cursor_options.grab_mode != CursorGrabMode::None {
            let window_scale = window.height().min(window.width());
            pitch -= (settings.sensitivity * ev.delta.y * window_scale / 10_000.0).to_radians();
            yaw -= (settings.sensitivity * ev.delta.x * window_scale / 10_000.0).to_radians();
        }

        pitch = pitch.clamp(-1.54, 1.54);

        camera.rotation = Quat::from_axis_angle(Vec3::X, pitch);
        player.rotation = Quat::from_axis_angle(Vec3::Y, yaw);
    }
}

#[derive(Component, Debug, Default)]
pub struct Velocity(pub Vec3);

fn player_movement(
    player: Single<(&mut Transform, &mut Velocity), With<Player>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    settings: Res<GameSettings>,
    game_info: Res<GameInfo>,
    time: Res<Time>,
) {
    let (mut transform, mut velocity) = player.into_inner();

    let delta = time.delta_secs();

    let mut move_dir = Vec3::ZERO;
    let mut sprint_multiplier = 1.0;

    let local_z = transform.local_z();

    let forward = -Vec3::new(local_z.x, 0., local_z.z).normalize_or_zero();
    let right = Vec3::new(local_z.z, 0., -local_z.x).normalize_or_zero();

    let should_jump = keyboard.pressed(KeyCode::Space);
    let sneaking = keyboard.pressed(KeyCode::ShiftLeft);

    if keyboard.pressed(KeyCode::KeyW) {
        if !sneaking && keyboard.pressed(KeyCode::ControlLeft) {
            sprint_multiplier = 1.6;
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

    let mut target_velocity = vec3(
        move_dir.x * settings.movement_speed * sprint_multiplier,
        0.0,
        move_dir.z * settings.movement_speed * sprint_multiplier,
    );

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
        let intended_move_x = Vec3::new(target_velocity.x * delta, 0.0, 0.0);
        let collision_ray_direction_x = intended_move_x.normalize_or_zero();
        let ray_check_distance_x = intended_move_x.length() + 0.05;

        for pos_offset in movement_collision_offsets {
            let ray_origin_for_collision = transform.translation + *pos_offset + Vec3::Y * 0.01;
            if let Some(hit) = ray_cast(
                &game_info,
                ray_origin_for_collision,
                collision_ray_direction_x,
                ray_check_distance_x,
            ) && hit.normal.as_vec3().dot(collision_ray_direction_x) < -0.1
            {
                target_velocity.x = 0.0;
                break;
            }
        }
    }

    if target_velocity.z != 0.0 {
        let intended_move_z = Vec3::new(0.0, 0.0, target_velocity.z * delta);
        let collision_ray_direction_z = intended_move_z.normalize_or_zero();
        let ray_check_distance_z = intended_move_z.length() + 0.05;

        for pos_offset in movement_collision_offsets {
            let ray_origin_for_collision = transform.translation + *pos_offset + Vec3::Y * 0.01;
            if let Some(hit) = ray_cast(
                &game_info,
                ray_origin_for_collision,
                collision_ray_direction_z,
                ray_check_distance_z,
            ) && hit.normal.as_vec3().dot(collision_ray_direction_z) < -0.1
            {
                target_velocity.z = 0.0;
                break;
            }
        }
    }

    if sneaking {
        target_velocity *= 0.5;
        if ray_cast(&game_info, transform.translation, -Vec3::Y, 0.2).is_none() {
            // TODO
        }
    }

    velocity.0.x = target_velocity.x;
    velocity.0.z = target_velocity.z;

    let mut grounded = false;
    let mut closest_ground_distance = f32::MAX;

    let grounded_collision_offsets = &[
        vec3(0.25, 0.1, 0.25),
        vec3(-0.25, 0.1, 0.25),
        vec3(0.25, 0.1, -0.25),
        vec3(-0.25, 0.1, -0.25),
        vec3(0.0, 0.1, 0.0),
    ];

    for offset in grounded_collision_offsets {
        if let Some(hit) = ray_cast(&game_info, transform.translation + offset, -Vec3::Y, 0.2) {
            grounded = true;

            if hit.distance < closest_ground_distance {
                closest_ground_distance = hit.distance;
            }
        }
    }

    if grounded {
        if should_jump {
            let mut hit = false;

            for offset in grounded_collision_offsets {
                if ray_cast(
                    &game_info,
                    transform.translation + Vec3::Y * 1.8 + offset,
                    Vec3::Y,
                    0.3,
                )
                .is_some()
                {
                    hit = true;
                    break;
                }
            }
            if hit {
                velocity.0.y = settings.jump_force / 4.0;
            } else {
                velocity.0.y = settings.jump_force;
            }
        } else {
            velocity.0.y = 0.0;
        }

        if velocity.0.y <= 0.0 && closest_ground_distance > 0.0 && closest_ground_distance < 0.1 {
            transform.translation.y -= closest_ground_distance - 0.1;
        }
    } else {
        velocity.0.y -= settings.gravity * delta;
    }

    transform.translation += velocity.0 * delta;
}
