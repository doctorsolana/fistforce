//! Server-side building placement and management
//!
//! Handles building placement requests, validates resources, applies terrain modifications,
//! and spawns building entities.

use bevy::prelude::*;
use lightyear::prelude::*;
use lightyear::prelude::server::*;
use std::collections::HashMap;

use shared::{
    BuildingType, PlaceBuildingRequest, PlacedBuilding, BuildingPosition,
    Inventory, WorldTerrain, Player, ChunkCoord, TerrainDeltaChunk,
    structures::{generate_medieval_town, MedievalTownBuilding, MEDIEVAL_SPACING},
    terrain::WORLD_SEED,
};

use crate::npc;

/// Resource to track if test buildings have been spawned
#[derive(Resource)]
pub struct TestBuildingsSpawned;

/// Resource mapping chunk coordinates to their delta chunk entities
/// Used to upsert delta chunks on terrain modification
#[derive(Resource, Default)]
pub struct DeltaChunkEntities {
    pub map: HashMap<ChunkCoord, Entity>,
}

/// Handle building placement requests from clients
pub fn handle_place_building_requests(
    mut commands: Commands,
    mut terrain: ResMut<WorldTerrain>,
    mut delta_entities: ResMut<DeltaChunkEntities>,
    mut clients: Query<(
        Entity,
        &RemoteId,
        &mut MessageReceiver<PlaceBuildingRequest>,
    ), With<ClientOf>>,
    mut player_inventories: Query<(&Player, &mut Inventory)>,
    mut delta_query: Query<&mut TerrainDeltaChunk>,
) {
    for (_client_entity, remote_id, mut receiver) in clients.iter_mut() {
        for request in receiver.receive() {
            info!(
                "Received PlaceBuildingRequest: {:?} at {:?}",
                request.building_type, request.position
            );
            
            // Find the player entity for this client
            let peer_id = remote_id.0;
            let player_result = player_inventories.iter_mut().find(|(player, _)| {
                player.client_id == peer_id
            });
            
            let Some((_, mut inventory)) = player_result else {
                warn!("Client has no player entity for building placement");
                continue;
            };
            
            let def = request.building_type.definition();
            
            // Check if player has required resources
            let can_afford = def.cost.iter().all(|(item_type, required)| {
                inventory.count_item(*item_type) >= *required
            });
            
            if !can_afford {
                info!("Player cannot afford building: {:?}", request.building_type);
                continue;
            }
            
            // Validate position (basic checks)
            let terrain_height = terrain.get_height(request.position.x, request.position.z);
            if (request.position.y - terrain_height).abs() > 5.0 {
                info!("Building position too far from terrain");
                continue;
            }
            
            // Deduct resources from inventory
            for (item_type, quantity) in def.cost {
                inventory.remove_item(*item_type, *quantity);
            }
            
            info!(
                "Building {:?} placed! Deducted resources.",
                request.building_type
            );
            
            // Add terrain modifications for flattening with rotation support
            let half_extents = Vec2::new(def.footprint.x / 2.0, def.footprint.y / 2.0);
            let building_pos = Vec3::new(request.position.x, terrain_height, request.position.z);
            
            let affected_chunks = terrain.apply_flatten_rect(
                building_pos,
                half_extents,
                request.rotation,
                def.flatten_radius,
            );
            
            info!(
                "Terrain modified: {} chunks affected, version {}",
                affected_chunks.len(),
                terrain.modification_version()
            );
            
            // Upsert TerrainDeltaChunk entities for affected chunks
            for coord in &affected_chunks {
                if let Some(delta_data) = terrain.get_delta_chunk(*coord) {
                    let chunk_component = TerrainDeltaChunk::from_delta_data(*coord, delta_data);
                    
                    if let Some(&existing_entity) = delta_entities.map.get(coord) {
                        // Update existing entity
                        if let Ok(mut existing) = delta_query.get_mut(existing_entity) {
                            *existing = chunk_component;
                        }
                    } else {
                        // Spawn new entity
                        let entity = commands.spawn((
                            chunk_component,
                            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                        )).id();
                        delta_entities.map.insert(*coord, entity);
                    }
                }
            }
            
            // Spawn the building entity
            let building_entity = commands.spawn((
                PlacedBuilding {
                    building_type: request.building_type,
                    rotation: request.rotation,
                },
                BuildingPosition(building_pos),
                Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
            )).id();
            
            info!(
                "Spawned building entity {:?} at {:?}",
                building_entity, building_pos
            );
        }
    }
}

/// Spawn a test building near spawn for verification
pub fn spawn_test_building(
    mut commands: Commands,
    spawned: Option<Res<TestBuildingsSpawned>>,
    mut terrain: ResMut<WorldTerrain>,
    mut delta_entities: ResMut<DeltaChunkEntities>,
    server_query: Query<Entity, (With<crate::GameServer>, With<Started>)>,
) {
    // Only spawn once and only after server is started
    if spawned.is_some() || server_query.is_empty() {
        return;
    }
    
    commands.insert_resource(TestBuildingsSpawned);
    
    // Spawn a train station at a fixed location near spawn
    let station_pos = Vec3::new(25.0, 0.0, 25.0);
    let terrain_height = terrain.get_height(station_pos.x, station_pos.z);
    let building_pos = Vec3::new(station_pos.x, terrain_height, station_pos.z);
    
    let def = BuildingType::TrainStation.definition();
    
    // Add terrain modifications with rotation support
    let half_extents = Vec2::new(def.footprint.x / 2.0, def.footprint.y / 2.0);
    let affected_chunks = terrain.apply_flatten_rect(
        building_pos,
        half_extents,
        0.0, // No rotation for test building
        def.flatten_radius,
    );
    
    info!("Test building flattened {} chunks", affected_chunks.len());
    
    // Spawn TerrainDeltaChunk entities for affected chunks
    for coord in &affected_chunks {
        if let Some(delta_data) = terrain.get_delta_chunk(*coord) {
            let chunk_component = TerrainDeltaChunk::from_delta_data(*coord, delta_data);
            let entity = commands.spawn((
                chunk_component,
                Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
            )).id();
            delta_entities.map.insert(*coord, entity);
        }
    }
    
    // Spawn the building
    commands.spawn((
        PlacedBuilding {
            building_type: BuildingType::TrainStation,
            rotation: 0.0,
        },
        BuildingPosition(building_pos),
        Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
    ));
    
    info!("Spawned test Train Station at {:?}", building_pos);
}

/// Resource to track if the medieval town has been spawned
#[derive(Resource)]
pub struct MedievalTownSpawned;

/// Spawn the medieval town with buildings and NPCs
pub fn spawn_medieval_town(
    mut commands: Commands,
    spawned: Option<Res<MedievalTownSpawned>>,
    mut terrain: ResMut<WorldTerrain>,
    mut delta_entities: ResMut<DeltaChunkEntities>,
    server_query: Query<Entity, (With<crate::GameServer>, With<Started>)>,
) {
    // Only spawn once and only after server is started
    if spawned.is_some() || server_query.is_empty() {
        return;
    }

    commands.insert_resource(MedievalTownSpawned);

    // Town center position - in grassland biome, away from desert and player spawn
    // Located at (350, terrain_height, 350)
    let town_center_x = 350.0;
    let town_center_z = 350.0;
    let terrain_height = terrain.get_height(town_center_x, town_center_z);
    let town_center = Vec3::new(town_center_x, terrain_height, town_center_z);

    info!("Spawning medieval town at {:?}", town_center);

    // First, flatten the entire town area to create a smooth foundation
    // Town spans 3x3 blocks, so total radius is ~1.5 * SPACING from center
    let town_radius = MEDIEVAL_SPACING * 1.7; // ~90m radius for the whole town
    let town_half_extents = Vec2::new(town_radius, town_radius);

    // Apply a large-scale flatten to the whole town area
    // This creates a gently leveled foundation without being perfectly flat
    let town_flatten_chunks = terrain.apply_flatten_rect(
        town_center,
        town_half_extents,
        0.0, // No rotation for overall town area
        15.0, // Gentle slope transition at edges
    );

    info!(
        "Town area flattened: {} chunks affected, radius {:.0}m",
        town_flatten_chunks.len(),
        town_radius
    );

    // Spawn delta chunk entities for town-wide flattening
    for coord in &town_flatten_chunks {
        if let Some(delta_data) = terrain.get_delta_chunk(*coord) {
            let chunk_component = TerrainDeltaChunk::from_delta_data(*coord, delta_data);
            if !delta_entities.map.contains_key(coord) {
                let entity = commands.spawn((
                    chunk_component,
                    Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                )).id();
                delta_entities.map.insert(*coord, entity);
            }
        }
    }

    // Generate town buildings
    let buildings = generate_medieval_town(town_center, WORLD_SEED);
    info!("Generated {} medieval town buildings", buildings.len());

    let mut total_chunks_affected = town_flatten_chunks.len();

    // Spawn each building with additional local terrain flattening
    for MedievalTownBuilding { building_type, position, rotation } in buildings {
        let def = building_type.definition();

        // Get terrain height at building position (now on flattened ground)
        let building_terrain_y = terrain.get_height(position.x, position.z);
        let building_pos = Vec3::new(position.x, building_terrain_y, position.z);

        // Apply local terrain flattening for this building
        let half_extents = Vec2::new(def.footprint.x / 2.0, def.footprint.y / 2.0);
        let affected_chunks = terrain.apply_flatten_rect(
            building_pos,
            half_extents,
            rotation,
            def.flatten_radius,
        );

        total_chunks_affected += affected_chunks.len();

        // Spawn/update TerrainDeltaChunk entities for affected chunks
        for coord in &affected_chunks {
            if let Some(delta_data) = terrain.get_delta_chunk(*coord) {
                let chunk_component = TerrainDeltaChunk::from_delta_data(*coord, delta_data);

                if !delta_entities.map.contains_key(coord) {
                    // Spawn new entity
                    let entity = commands.spawn((
                        chunk_component,
                        Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                    )).id();
                    delta_entities.map.insert(*coord, entity);
                }
            }
        }

        // Spawn the building entity
        commands.spawn((
            PlacedBuilding {
                building_type,
                rotation,
            },
            BuildingPosition(building_pos),
            Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
        ));
    }

    info!(
        "Medieval town buildings spawned, {} terrain chunks modified total",
        total_chunks_affected
    );

    // Spawn NPCs for the town
    let mut npc_id_start = 1000_u64; // Start IDs at 1000 to avoid conflicts with other NPCs
    npc::spawn_medieval_town_npcs(&mut commands, &terrain, town_center, &mut npc_id_start);
}
