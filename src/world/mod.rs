use bevy::{pbr::wireframe::WireframePlugin, prelude::*, tasks::Task};
use ferriscraft::{Block, GameEntity};

use crate::{
    GameSettings,
    ui::GameState,
    world::{
        mesher::ChunkMesh,
        systems::{
            autosave_and_exit, handle_chunk_despawn, handle_chunk_gen, handle_mesh_gen,
            process_tasks,
        },
    },
};

pub mod utils;

mod mesher;
mod systems;

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(WireframePlugin::default())
            .add_systems(Update, autosave_and_exit)
            .add_systems(
                Update,
                (
                    handle_chunk_gen,
                    handle_mesh_gen,
                    handle_chunk_despawn
                        .run_if(|game_settings: Res<GameSettings>| game_settings.despawn_chunks),
                    process_tasks,
                )
                    .run_if(in_state(GameState::MultiPlayer).or(in_state(GameState::SinglePlayer))),
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

#[derive(Component)]
struct ComputeChunk(Task<Chunk>, IVec3);

#[derive(Component)]
struct ComputeChunkMesh(Task<Option<ChunkMesh>>, IVec3);
