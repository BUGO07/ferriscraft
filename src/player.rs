use crate::{GameInfo, GameSettings, utils::ray_cast};
use bevy::{
    input::mouse::MouseMotion,
    prelude::*,
    window::{CursorGrabMode, PrimaryWindow},
};

#[derive(Component)]
pub struct Player;

#[derive(Component)]
pub struct PlayerCamera;

pub fn camera_movement(
    mut camera: Single<&mut Transform, With<PlayerCamera>>,
    mut mouse: EventReader<MouseMotion>,
    settings: Res<GameSettings>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    for ev in mouse.read() {
        let (mut yaw, mut pitch, _) = camera.rotation.to_euler(EulerRot::YXZ);
        match window.cursor_options.grab_mode {
            CursorGrabMode::None => (),
            _ => {
                let window_scale = window.height().min(window.width());
                pitch -= (settings.sensitivity * ev.delta.y * window_scale / 10_000.0).to_radians();
                yaw -= (settings.sensitivity * ev.delta.x * window_scale / 10_000.0).to_radians();
            }
        }

        pitch = pitch.clamp(-1.54, 1.54);

        camera.rotation =
            Quat::from_axis_angle(Vec3::Y, yaw) * Quat::from_axis_angle(Vec3::X, pitch);
    }
}

#[derive(Component, Debug, Default)]
pub struct Velocity(pub Vec3);

pub fn player_movement(
    player: Single<(&mut Transform, &mut Velocity), (With<Player>, Without<PlayerCamera>)>,
    game_info: ResMut<GameInfo>,
    camera: Single<&Transform, (With<PlayerCamera>, Without<Player>)>,
    keyboard: Res<ButtonInput<KeyCode>>,
    settings: Res<GameSettings>,
    time: Res<Time>,
) {
    let (mut transform, mut velocity) = player.into_inner();

    let mut dir = Vec3::ZERO;
    let mut jump = false;
    let mut sprint = 1.0;

    let local_z = camera.local_z();

    let forward = -Vec3::new(local_z.x, 0., local_z.z).normalize_or_zero();
    let right = Vec3::new(local_z.z, 0., -local_z.x).normalize_or_zero();

    if keyboard.pressed(KeyCode::KeyW) {
        dir += forward;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        dir -= forward;
    }
    if keyboard.pressed(KeyCode::KeyA) {
        dir -= right;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        dir += right;
    }
    if keyboard.pressed(KeyCode::Space) {
        jump = true;
    }
    if keyboard.pressed(KeyCode::ControlLeft) {
        sprint = 1.6;
    }

    let move_direction = dir.normalize_or_zero();

    let mut target_velocity_x = move_direction.x * settings.movement_speed * sprint;
    let mut target_velocity_z = move_direction.z * settings.movement_speed * sprint;

    let collision_points_offsets = &[
        vec3(0.25, 0.0, 0.25),
        vec3(-0.25, 0.0, 0.25),
        vec3(0.25, 0.0, -0.25),
        vec3(-0.25, 0.0, -0.25),
        vec3(0.0, 0.0, 0.0),
    ];

    if target_velocity_x.abs() != 0.0 {
        let intended_move_x = Vec3::new(target_velocity_x * time.delta_secs(), 0.0, 0.0);
        let collision_ray_direction_x = intended_move_x.normalize_or_zero();
        let ray_check_distance_x = intended_move_x.length() + 0.05;

        let mut x_collision = false;
        for pos_offset in collision_points_offsets {
            let ray_origin_for_collision = transform.translation + *pos_offset + Vec3::Y * 0.01;
            if let Some(hit) = ray_cast(
                &game_info,
                ray_origin_for_collision,
                collision_ray_direction_x,
                ray_check_distance_x,
            ) && hit.distance < ray_check_distance_x
                && hit.normal.as_vec3().dot(collision_ray_direction_x) < -0.1
            {
                x_collision = true;
                break;
            }
        }
        if x_collision {
            target_velocity_x = 0.0;
        }
    }

    if target_velocity_z.abs() != 0.0 {
        let intended_move_z = Vec3::new(0.0, 0.0, target_velocity_z * time.delta_secs());
        let collision_ray_direction_z = intended_move_z.normalize_or_zero();
        let ray_check_distance_z = intended_move_z.length() + 0.05;

        let mut z_collision = false;
        for pos_offset in collision_points_offsets {
            let ray_origin_for_collision = transform.translation + *pos_offset + Vec3::Y * 0.01;
            if let Some(hit) = ray_cast(
                &game_info,
                ray_origin_for_collision,
                collision_ray_direction_z,
                ray_check_distance_z,
            ) && hit.distance < ray_check_distance_z
                && hit.normal.as_vec3().dot(collision_ray_direction_z) < -0.1
            {
                z_collision = true;
                break;
            }
        }
        if z_collision {
            target_velocity_z = 0.0;
        }
    }

    velocity.0.x = target_velocity_x;
    velocity.0.z = target_velocity_z;

    let ray_origin = transform.translation + Vec3::Y * 0.1;
    let ray_direction = Vec3::new(0.0, -1.0, 0.0);

    let mut grounded = false;
    let mut closest_ground_distance = f32::MAX;

    for pos_offset in &[
        vec3(0.25, 0.0, -0.25),
        vec3(0.25, 0.0, 0.25),
        vec3(-0.25, 0.0, -0.25),
        vec3(-0.25, 0.0, 0.25),
    ] {
        if let Some(hit) = ray_cast(&game_info, ray_origin + pos_offset, ray_direction, 1.0) {
            if hit.distance < 0.1 {
                grounded = true;
            }

            if hit.distance < closest_ground_distance {
                closest_ground_distance = hit.distance;
            }
        }
    }

    if !game_info.loaded {
        if transform.translation.y < 0.1 {
            transform.translation.y = 0.0;
        }
    } else if grounded {
        if jump {
            if let Some(hit) = ray_cast(
                &game_info,
                transform.translation + Vec3::Y * 1.8,
                Vec3::Y,
                1.0,
            ) {
                if hit.distance < 0.1 {
                    velocity.0.y = settings.jump_force;
                }
            } else {
                velocity.0.y = settings.jump_force;
            }
        } else {
            velocity.0.y = 0.0;
        }

        if velocity.0.y <= 0.0 && closest_ground_distance > 0.0 && closest_ground_distance < 0.1 {
            transform.translation.y = ray_origin.y - closest_ground_distance;
        }
    } else {
        velocity.0.y -= settings.gravity * time.delta_secs();
    }

    transform.translation += velocity.0 * time.delta_secs();
}

pub fn toggle_grab_cursor(window: &mut Window) {
    match window.cursor_options.grab_mode {
        CursorGrabMode::None => {
            window.cursor_options.grab_mode = CursorGrabMode::Confined;
            window.cursor_options.visible = false;
        }
        _ => {
            window.cursor_options.grab_mode = CursorGrabMode::None;
            window.cursor_options.visible = true;
        }
    }
}
