use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages,
    diagnostic::{EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin},
    prelude::*,
    render::mesh::{Indices, PrimitiveTopology},
    window::WindowMode,
};
use bevy_flycam::{FlyCam, MovementSettings, NoCameraPlayerPlugin};
use bevy_inspector_egui::{bevy_egui::EguiPlugin, quick::WorldInspectorPlugin};
use iyes_perf_ui::{PerfUiPlugin, prelude::PerfUiAllEntries};
use noiz::{
    Noise,
    prelude::common_noise::{Perlin, Simplex},
    rng::NoiseRng,
};

use crate::{
    mesher::Chunk,
    utils::{Block, BlockKind, get_vertex_u32, kind2color, noise, vec3_to_index},
};

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
        .add_systems(Startup, setup)
        .add_systems(Update, (handle_chunks, generate_chunk_mesh))
        .run();
}

#[derive(Resource)]
pub struct GameInfo {
    pub simplex: Noise<Simplex>,
    pub perlin: Noise<Perlin>,
    pub chunks: HashMap<IVec3, Chunk>,
    pub materials: Vec<Handle<StandardMaterial>>,
}

pub const CHUNK_SIZE: i32 = 16; // MAX 63
pub const CHUNK_HEIGHT: i32 = 256; // MAX 511
pub const SEA_LEVEL: i32 = 64;
pub const RENDER_DISTANCE: i32 = 8;

fn setup(
    mut commands: Commands,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    let seed = 0;
    commands.insert_resource(GameInfo {
        simplex: Noise {
            frequency: 0.00420,
            noise: Simplex::default(),
            seed: NoiseRng(seed),
        },
        perlin: Noise {
            frequency: 0.0069,
            noise: Perlin::default(),
            seed: NoiseRng(seed),
        },
        chunks: HashMap::new(),
        materials: vec![
            materials.add(StandardMaterial {
                base_color: Color::srgb(1.0, 1.0, 1.0),
                base_color_texture: Some(asset_server.load("atlas.png")),
                ..default()
            }),
            materials.add(kind2color(BlockKind::Air)),
            materials.add(kind2color(BlockKind::Stone)),
            materials.add(kind2color(BlockKind::Dirt)),
            materials.add(kind2color(BlockKind::Grass)),
            materials.add(kind2color(BlockKind::Plank)),
        ],
    });

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
}

fn handle_chunks(
    mut commands: Commands,
    mut game_info: ResMut<GameInfo>,
    // chunks: Query<(Entity, &Chunk)>,
    player: Single<&Transform, With<Camera3d>>,
) {
    let pt = player.translation;
    // TODO
    // for chunk in game_info.chunks.clone().keys() {
    //     // check if its far from the player and if it is despawn it
    //     if (chunk.x + RENDER_DISTANCE < pt.x as i32 / CHUNK_SIZE)
    //         || (chunk.x - RENDER_DISTANCE > pt.x as i32 / CHUNK_SIZE)
    //         || (chunk.z + RENDER_DISTANCE < pt.z as i32 / CHUNK_SIZE)
    //         || (chunk.z - RENDER_DISTANCE > pt.z as i32 / CHUNK_SIZE)
    //     {
    //         commands
    //             .entity(chunks.iter().find(|x| x.1.pos == *chunk).unwrap().0)
    //             .despawn();
    //         game_info.chunks.remove(chunk);
    //     }
    // }
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
                pos: ivec3(chunk_x, 0, chunk_z),
                blocks: vec![
                    Block {
                        kind: BlockKind::Air
                    };
                    (CHUNK_SIZE * CHUNK_SIZE * CHUNK_HEIGHT) as usize
                ],
            };
            for rela_z in 0..CHUNK_SIZE {
                for rela_x in 0..CHUNK_SIZE {
                    // [-1.0 .. 1.0] -> [0.0 .. 2.0] -> [0 .. CHUNK_HEIGHT - SEA_LEVEL] + SEA_LEVEL
                    let max_y = (noise(
                        &game_info.simplex,
                        // &game_info.perlin,
                        vec2(
                            (rela_x + chunk_x * CHUNK_SIZE) as f32,
                            (rela_z + chunk_z * CHUNK_SIZE) as f32,
                        ),
                    )
                    .powf(1.5)
                        / 3.0
                        * (CHUNK_HEIGHT - SEA_LEVEL) as f32) as i32
                        + SEA_LEVEL;

                    for y in 0..CHUNK_HEIGHT {
                        // above 105 blocks its mountainy and so its stone
                        // if its below 105 the top block is grass, 4 blocks below is dirt, and the rest below is stone (minecraft style)
                        chunk.blocks[vec3_to_index(ivec3(rela_x, y, rela_z))] = Block {
                            kind: if y > max_y {
                                BlockKind::Air
                            } else if y > 105 {
                                BlockKind::Stone
                            } else if y == max_y {
                                BlockKind::Grass
                            } else if y < max_y && y > max_y - 5 {
                                BlockKind::Dirt
                            } else {
                                BlockKind::Stone
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
            let (pos, _ao, normal, _block_type) = get_vertex_u32(vertex);
            positions.push(pos);
            normals.push(normal);
        }

        bevy_mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        bevy_mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        bevy_mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, mesh.uvs);

        bevy_mesh.insert_indices(Indices::U32(mesh.indices.clone()));
        let mesh_handle = meshes.add(bevy_mesh);
        if let Ok(mut e) = commands.get_entity(entity) {
            e.try_insert((
                Mesh3d(mesh_handle),
                MeshMaterial3d(game_info.materials[0].clone()),
                Visibility::Visible,
            ));
        }
    }
}
