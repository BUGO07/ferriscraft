use bevy::{
    diagnostic::{EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin},
    input::common_conditions::input_just_pressed,
    platform::collections::HashMap,
    prelude::*,
    window::WindowMode,
};
use bevy_flycam::{FlyCam, NoCameraPlayerPlugin};
use bevy_inspector_egui::{bevy_egui::EguiPlugin, quick::WorldInspectorPlugin};
use iyes_perf_ui::{PerfUiPlugin, prelude::PerfUiAllEntries};
use noiz::{Noise, SampleableFor, prelude::common_noise::Perlin, rng::NoiseRng};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "FerrisCraft".to_string(),
                mode: WindowMode::Windowed,
                ..default()
            }),
            ..default()
        }))
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins(EntityCountDiagnosticsPlugin)
        .add_plugins(PerfUiPlugin)
        .add_plugins(EguiPlugin {
            enable_multipass_for_primary_context: false,
        })
        .add_plugins(WorldInspectorPlugin::default())
        .add_plugins(NoCameraPlayerPlugin)
        .insert_resource(GameInfo {
            noise: Noise {
                frequency: 0.0069 * 2.0,
                noise: Perlin::default(),
                seed: NoiseRng(0),
            },
            chunks: HashMap::new(),
        })
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            ((spawn_chunks, populate_chunks)
                .chain()
                .run_if(input_just_pressed(KeyCode::KeyH)),),
        )
        .run();
}

#[derive(Component, Clone, Copy)]
pub struct Block {
    pub kind: BlockKind,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Air = 0x00000000,
    Grass = 0xFF00FF00,
    Dirt = 0xFFA52A2A,
    Stone = 0xFF9E9E9E,
}

pub fn kind2color(kind: BlockKind) -> Color {
    let color = kind as u32;
    let b = color & 0xFF;
    let g = (color >> 8) & 0xFF;
    let r = (color >> 16) & 0xFF;
    let a = (color >> 24) & 0xFF;
    Color::srgba_u8(r as u8, g as u8, b as u8, a as u8)
}

pub fn is_transparent(kind: BlockKind) -> bool {
    kind == BlockKind::Air
    // let color = kind as u32;
    // let a = (color >> 24) & 0xFF;
    // a != 0xFF
}

#[derive(Component, Clone)]
pub struct Chunk {
    pub x: isize,
    pub z: isize,
    // x y z
    pub blocks: HashMap<(isize, isize, isize), Block>,
}

#[derive(Resource)]
pub struct GameInfo {
    noise: Noise<Perlin>,
    // x z
    chunks: HashMap<(isize, isize), Chunk>,
}

pub const CHUNK_SIZE: isize = 16;
pub const CHUNK_HEIGHT: isize = 64;
pub const SEA_LEVEL: isize = 8;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        FlyCam,
        Transform::from_xyz(5.0, CHUNK_HEIGHT as f32, -5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn(PerfUiAllEntries::default());
    commands.spawn((
        PointLight {
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0),
    ));

    commands.spawn((
        Mesh3d(meshes.add(Circle::new(4.0))),
        MeshMaterial3d(materials.add(Color::WHITE)),
        Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(Color::srgb_u8(124, 144, 255))),
        Transform::from_xyz(0.0, 0.5, 0.0),
    ));
}

fn spawn_chunks(mut commands: Commands, mut game_info: ResMut<GameInfo>) {
    for chunk_z in -25..25 {
        for chunk_x in -25..25 {
            let mut chunk = Chunk {
                x: chunk_x,
                z: chunk_z,
                blocks: HashMap::new(),
            };
            for rela_z in 0..CHUNK_SIZE {
                for rela_x in 0..CHUNK_SIZE {
                    let max_y_float: f32 = game_info.noise.sample(Vec2::new(
                        (rela_x + chunk_x * CHUNK_SIZE) as f32,
                        (rela_z + chunk_z * CHUNK_SIZE) as f32,
                    ));
                    // [-1.0 .. 1.0] -> [0.0 .. 2.0] -> [0 .. CHUNK_HEIGHT - SEA_LEVEL] + SEA_LEVEL
                    let max_y = ((max_y_float + 1.0) * (CHUNK_HEIGHT - SEA_LEVEL) as f32 / 2.0)
                        as isize
                        + SEA_LEVEL;

                    for y in 0..CHUNK_HEIGHT {
                        chunk.blocks.insert(
                            (rela_x, y, rela_z),
                            Block {
                                kind: if y > max_y {
                                    BlockKind::Air
                                } else {
                                    BlockKind::Dirt
                                },
                            },
                        );
                    }
                }
            }
            game_info.chunks.insert((chunk_x, chunk_z), chunk.clone());
            commands.spawn((
                Name::new(format!("CHUNK ({chunk_x}, {chunk_z})")),
                chunk,
                Transform::from_xyz(
                    chunk_x as f32 * CHUNK_SIZE as f32,
                    0.0,
                    chunk_z as f32 * CHUNK_SIZE as f32,
                ),
                Visibility::Visible,
            ));
        }
    }
}

fn populate_chunks(
    mut commands: Commands,
    chunks: Query<(Entity, &Chunk), Added<Chunk>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    game_info: Res<GameInfo>,
) {
    let quad = meshes.add(Mesh::from(Rectangle::new(1.0, 1.0)));
    for (entity, chunk) in chunks.iter() {
        for y in 0..CHUNK_HEIGHT {
            for rela_z in 0..CHUNK_SIZE {
                for rela_x in 0..CHUNK_SIZE {
                    let block = chunk.blocks.get(&(rela_x, y, rela_z)).unwrap();
                    if block.kind == BlockKind::Air {
                        continue;
                    }
                    let chunk_n = game_info.chunks.get(&(chunk.x, chunk.z - 1));
                    let chunk_s = game_info.chunks.get(&(chunk.x, chunk.z + 1));
                    let chunk_e = game_info.chunks.get(&(chunk.x + 1, chunk.z));
                    let chunk_w = game_info.chunks.get(&(chunk.x - 1, chunk.z));

                    let get_block = |mut x: isize, y: isize, mut z: isize| -> Option<&Block> {
                        if !(0..CHUNK_HEIGHT).contains(&y) {
                            return None;
                        }

                        let mut current_chunk = Some(chunk);
                        if x < 0 {
                            x = CHUNK_SIZE - 1;
                            current_chunk = chunk_w;
                        } else if x >= CHUNK_SIZE {
                            x = 0;
                            current_chunk = chunk_e;
                        }
                        if z < 0 {
                            z = CHUNK_SIZE - 1;
                            current_chunk = chunk_n;
                        } else if z >= CHUNK_SIZE {
                            z = 0;
                            current_chunk = chunk_s;
                        }
                        current_chunk?.blocks.get(&(x, y, z))
                    };

                    let checkv = |block: Option<&Block>| -> bool {
                        block.is_none() || is_transparent(block.unwrap().kind)
                    };

                    let check = |block: Option<&Block>| -> bool {
                        block.is_some() && is_transparent(block.unwrap().kind)
                    };

                    let mut face_transforms: Vec<Transform> = Vec::new();

                    if checkv(get_block(rela_x, y + 1, rela_z)) {
                        // up
                        face_transforms
                            .push(Transform::from_translation(Vec3::Y * 0.5).with_rotation(
                                Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
                            ));
                    }

                    if checkv(get_block(rela_x, y - 1, rela_z)) {
                        // down
                        face_transforms.push(
                            Transform::from_translation(-Vec3::Y * 0.5)
                                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                        );
                    }

                    if check(get_block(rela_x, y, rela_z - 1)) {
                        // north
                        face_transforms.push(
                            Transform::from_translation(-Vec3::Z * 0.5)
                                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
                        );
                    }

                    if check(get_block(rela_x, y, rela_z + 1)) {
                        // south
                        face_transforms.push(Transform::from_translation(Vec3::Z * 0.5));
                    }

                    if check(get_block(rela_x - 1, y, rela_z)) {
                        // west
                        face_transforms
                            .push(Transform::from_translation(-Vec3::X * 0.5).with_rotation(
                                Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
                            ));
                    }

                    if check(get_block(rela_x + 1, y, rela_z)) {
                        // east
                        face_transforms.push(
                            Transform::from_translation(Vec3::X * 0.5)
                                .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
                        );
                    }

                    for transform in face_transforms.iter() {
                        commands.spawn((
                            Mesh3d(quad.clone()),
                            MeshMaterial3d(materials.add(kind2color(block.kind))),
                            (*transform).with_translation(
                                transform.translation
                                    + Vec3::new(rela_x as f32, y as f32, rela_z as f32),
                            ),
                            ChildOf(entity),
                        ));
                    }
                }
            }
        }
    }
}
