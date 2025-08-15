use bevy::{pbr::wireframe::WireframePlugin, prelude::*, tasks::Task};
use ferriscraft::{Block, GameEntity};

use crate::{
    GameInfo,
    ui::GameState,
    world::{
        mesher::ChunkMesh,
        systems::{
            autosave_and_exit, handle_chunk_despawn, handle_chunk_gen, handle_mesh_gen,
            process_tasks,
        },
    },
};

pub mod mesher;
pub mod systems;
pub mod utils;

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
                        .run_if(|game_info: Res<GameInfo>| game_info.settings.despawn_chunks),
                    process_tasks,
                )
                    .run_if(not(in_state(GameState::Menu))),
            )
            .add_systems(
                Update,
                |mut commands: Commands,
                 mut transitions: EventReader<StateTransitionEvent<GameState>>,
                 query: Query<Entity, With<ChunkMarker>>| {
                    let Some(transition) = transitions.read().last() else {
                        return;
                    };
                    if transition.entered == transition.exited {
                        return;
                    }
                    let Some(entered) = &transition.entered else {
                        return;
                    };
                    if *entered == GameState::Menu {
                        for entity in &query {
                            commands.entity(entity).despawn();
                        }
                    }
                },
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
pub struct ComputeChunk(pub Task<Chunk>, pub IVec3);

#[derive(Component)]
pub struct ComputeChunkMesh(pub Task<Option<ChunkMesh>>, pub IVec3);
