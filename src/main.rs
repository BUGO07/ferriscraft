use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages,
    diagnostic::{EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin},
    prelude::*,
    render::{
        mesh::{Indices, PrimitiveTopology},
        primitives::Aabb,
    },
    window::WindowMode,
};
use bevy_flycam::{FlyCam, MovementSettings, NoCameraPlayerPlugin};
use bevy_inspector_egui::{bevy_egui::EguiPlugin, quick::WorldInspectorPlugin};
use iyes_perf_ui::{PerfUiPlugin, prelude::PerfUiAllEntries};
use noiz::{Noise, SampleableFor, prelude::common_noise::Simplex, rng::NoiseRng};

use crate::{mesher::Direction, utils::vec3_to_index};

pub mod mesher;
pub mod utils;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "FerrisCraft".to_string(),
                    mode: WindowMode::Windowed,
                    ..default()
                }),
                ..default()
            }),
            FrameTimeDiagnosticsPlugin::default(),
            EntityCountDiagnosticsPlugin,
            PerfUiPlugin,
            EguiPlugin {
                enable_multipass_for_primary_context: false,
            },
            WorldInspectorPlugin::default(),
            NoCameraPlayerPlugin,
        ))
        .insert_resource(MovementSettings {
            speed: 100.0,
            ..default()
        })
        .insert_resource(GameInfo {
            noise: Noise {
                frequency: 0.00420,
                noise: Simplex::default(),
                seed: NoiseRng(0),
            },
            chunks: HashMap::new(),
            materials: Vec::new(),
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (handle_chunks, generate_chunk_mesh))
        .run();
}

#[derive(Component, Clone, Copy, Default, Debug)]
pub struct Block {
    pub kind: BlockKind,
}

#[repr(u32)]
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum BlockKind {
    #[default]
    Air,
    Grass,
    Dirt,
    Stone,
}

impl BlockKind {
    pub fn is_solid(self) -> bool {
        self != BlockKind::Air
    }
}

pub fn kind2color(kind: BlockKind) -> Color {
    let color: u32 = match kind {
        BlockKind::Air => 0x00000000,
        BlockKind::Grass => 0xFF119C13,
        BlockKind::Dirt => 0xFF915E34,
        BlockKind::Stone => 0xFFA39E99,
    };
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
    pub pos: IVec3,
    // x y z
    pub blocks: Vec<Block>,
}

impl Chunk {
    pub fn get_block(&self, pos: IVec3) -> &Block {
        let index = vec3_to_index(pos);
        if index < self.blocks.len() {
            &self.blocks[index]
        } else {
            &Block {
                kind: BlockKind::Air,
            }
        }
    }

    pub fn get_adjacent_blocks(
        &self,
        pos: IVec3,
        chunks: &HashMap<IVec3, Chunk>,
        // current back, left, down
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

    pub fn get_von_neumann(&self, pos: IVec3) -> Vec<(Direction, &Block)> {
        vec![
            (Direction::South, self.get_block(pos + ivec3(0, 0, -1))),
            (Direction::North, self.get_block(pos + ivec3(0, 0, 1))),
            (Direction::Bottom, self.get_block(pos + ivec3(0, -1, 0))),
            (Direction::Top, self.get_block(pos + ivec3(0, 1, 0))),
            (Direction::West, self.get_block(pos + ivec3(-1, 0, 0))),
            (Direction::East, self.get_block(pos + ivec3(1, 0, 0))),
        ]
    }
}

#[derive(Resource)]
pub struct GameInfo {
    noise: Noise<Simplex>,
    // x z
    chunks: HashMap<IVec3, Chunk>,
    materials: Vec<Handle<StandardMaterial>>,
}

pub const CHUNK_SIZE: i32 = 16;
pub const CHUNK_HEIGHT: i32 = 256;
pub const SEA_LEVEL: i32 = 64;
pub const RENDER_DISTANCE: i32 = 8;

fn setup(
    mut commands: Commands,
    mut game_info: ResMut<GameInfo>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.spawn((
        Camera3d::default(),
        FlyCam,
        Transform::from_xyz(5.0, CHUNK_HEIGHT as f32, -5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.spawn(PerfUiAllEntries::default());
    commands.spawn((
        DirectionalLight {
            illuminance: 5_000.0,
            shadows_enabled: true,
            ..default()
        },
        // light idk
        Transform::from_rotation(Quat::from_euler(
            EulerRot::ZYX,
            0.0,
            std::f32::consts::FRAC_PI_4,  // 45 degrees around Y
            -std::f32::consts::FRAC_PI_3, // -60 degrees pitch (sun in sky)
        )),
    ));

    game_info.materials = vec![
        materials.add(kind2color(BlockKind::Air)),
        materials.add(kind2color(BlockKind::Grass)),
        materials.add(kind2color(BlockKind::Dirt)),
        materials.add(kind2color(BlockKind::Stone)),
    ]
}

// I DONT FUCKING KNOW HOW TO MAKE IT BETTER SO IT IS WHAT IT IS
fn noise(noise: &Noise<Simplex>, pos: Vec2) -> f32 {
    let n: f32 = noise.sample(pos);
    ((n + 1.0) / 2.0).powf(3.5) / 3.0 * (CHUNK_HEIGHT - SEA_LEVEL) as f32
}

fn handle_chunks(
    mut commands: Commands,
    mut game_info: ResMut<GameInfo>,
    player: Single<&Transform, With<Camera3d>>,
) {
    let pt = player.translation;
    for chunk_z in
        (pt.z as i32 / CHUNK_SIZE - RENDER_DISTANCE)..(pt.z as i32 / CHUNK_SIZE + RENDER_DISTANCE)
    {
        for chunk_x in (pt.x as i32 / CHUNK_SIZE - RENDER_DISTANCE)
            ..(pt.x as i32 / CHUNK_SIZE + RENDER_DISTANCE)
        {
            if game_info.chunks.contains_key(&ivec3(chunk_x, 0, chunk_z)) {
                continue;
            }
            let mut chunk = Chunk {
                pos: IVec3::new(chunk_x, 0, chunk_z),
                blocks: vec![
                    Block {
                        kind: BlockKind::Air
                    };
                    (CHUNK_SIZE * CHUNK_SIZE * CHUNK_HEIGHT) as usize
                ],
            };
            for rela_z in 0..CHUNK_SIZE {
                for rela_x in 0..CHUNK_SIZE {
                    // [-1.0 .. 1.0] -> [0.0 .. 2.0] -> [0 .. CHUNK_HEIGHT]
                    let max_y = noise(
                        &game_info.noise,
                        Vec2::new(
                            (rela_x + chunk_x * CHUNK_SIZE) as f32,
                            (rela_z + chunk_z * CHUNK_SIZE) as f32,
                        ),
                    ) as i32
                        + SEA_LEVEL;

                    for y in 0..CHUNK_HEIGHT {
                        chunk.blocks[vec3_to_index(IVec3::new(rela_x, y, rela_z))] = Block {
                            kind: if y > max_y {
                                BlockKind::Air
                            } else {
                                BlockKind::Grass
                            },
                        };
                    }
                }
            }
            game_info
                .chunks
                .insert(ivec3(chunk_x, 0, chunk_z), chunk.clone());
            commands.spawn((
                Name::new(format!("CHUNK ({chunk_x}, {chunk_z})")),
                chunk,
                Transform::from_xyz(
                    (chunk_x * CHUNK_SIZE) as f32,
                    0.0,
                    (chunk_z * CHUNK_SIZE) as f32,
                ),
                Visibility::Visible,
            ));
        }
    }
}

fn generate_chunk_mesh(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    game_info: Res<GameInfo>,
    chunks: Query<(Entity, &Chunk), Added<Chunk>>,
) {
    for (entity, chunk) in chunks.iter() {
        let mesh = mesher::build_chunk_mesh(chunk, &game_info.chunks).unwrap();
        let mut bevy_mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        for &vertex in mesh.vertices.iter() {
            let x = (vertex & 0xF) as f32; // bits 0..3
            let y = ((vertex >> 4) & 0xFF) as f32; // bits 4..11
            let z = ((vertex >> 12) & 0xF) as f32; // bits 12..15
            // let ao = (vertex >> 16) & 0x7; // bits 16..18
            let normal_index = (vertex >> 19) & 0x7; // bits 19..21
            // let block_type = (vertex >> 22) & 0x3FF; // bits 22..31 (for uv textures in the future)

            positions.push([x, y, z]);
            normals.push(NORMALS[normal_index as usize]);
        }

        bevy_mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        bevy_mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        // bevy_mesh.set_indices(Some(Indices::U32(mesh.indices.clone().into())));
        bevy_mesh.insert_indices(Indices::U32(mesh.indices.clone()));
        let mesh_handle = meshes.add(bevy_mesh);
        commands.entity(entity).insert((
            Aabb::from_min_max(
                Vec3::ZERO,
                IVec3::new(CHUNK_SIZE, CHUNK_HEIGHT, CHUNK_SIZE).as_vec3(),
            ),
            Mesh3d(mesh_handle),
            MeshMaterial3d(game_info.materials[BlockKind::Grass as usize].clone()),
            // (*transform).with_translation(
            //     transform.translation + Vec3::new(rela_x as f32, y as f32, rela_z as f32),
            // ),
            Visibility::Visible,
            InheritedVisibility::default(),
            ViewVisibility::default(),
        ));
    }
}

const NORMALS: &[[f32; 3]; 6] = &[
    [-1.0, 0.0, 0.0], // West
    [1.0, 0.0, 0.0],  // East
    [0.0, -1.0, 0.0], // Bottom
    [0.0, 1.0, 0.0],  // Top
    [0.0, 0.0, -1.0], // South
    [0.0, 0.0, 1.0],  // North
];
