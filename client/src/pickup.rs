//! Pickup system - detect nearby ground items and allow pickup with E key
//!
//! Shows a prompt when near items and sends PickupRequest to server.
//! Also handles 3D visuals for ground items with bobbing animation.

use bevy::prelude::*;
use shared::{GroundItem, GroundItemPosition, LocalPlayer, PlayerPosition, PickupRequest, ReliableChannel, ItemType};
use lightyear::prelude::*;
use lightyear::prelude::client::Connected;

use crate::input::InputState;
use crate::states::GameState;

/// Plugin for the pickup system
pub struct PickupPlugin;

impl Plugin for PickupPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NearbyItem>();
        
        // Visual systems run in both Playing and Paused states (so items don't disappear when pausing)
        app.add_systems(Update, (
            spawn_ground_item_visuals,
            animate_ground_items,
            despawn_ground_item_visuals,
        ).chain().run_if(in_state(GameState::Playing).or(in_state(GameState::Paused))));
        
        // Pickup interaction only in Playing state
        app.add_systems(Update, (
            detect_nearby_items,
            show_pickup_prompt,
            handle_pickup_input,
        ).chain().run_if(in_state(GameState::Playing)));
        
        // Cleanup prompt when leaving Playing (but NOT item visuals - they persist through pause)
        app.add_systems(OnExit(GameState::Playing), cleanup_pickup_ui);
        
        // Only cleanup item visuals when returning to main menu (actual disconnect)
        app.add_systems(OnEnter(GameState::MainMenu), cleanup_item_visuals);
    }
}

/// Distance within which items can be picked up
const PICKUP_RANGE: f32 = 3.0;

/// Resource tracking the nearest item to the player
#[derive(Resource, Default)]
pub struct NearbyItem {
    pub entity: Option<Entity>,
    pub item_type: Option<shared::ItemType>,
    pub quantity: Option<u32>,
}

/// Marker for the pickup prompt UI
#[derive(Component)]
pub struct PickupPrompt;

/// Detect the nearest ground item to the local player
fn detect_nearby_items(
    mut nearby: ResMut<NearbyItem>,
    local_player: Query<&PlayerPosition, With<LocalPlayer>>,
    ground_items: Query<(Entity, &GroundItem, &GroundItemPosition)>,
    input_state: Res<InputState>,
) {
    // Don't detect while in vehicle or dead
    if input_state.in_vehicle || input_state.is_dead {
        *nearby = NearbyItem::default();
        return;
    }
    
    let Ok(player_pos) = local_player.single() else {
        *nearby = NearbyItem::default();
        return;
    };
    
    // Find the nearest item within pickup range
    let mut closest: Option<(Entity, &GroundItem, f32)> = None;
    
    for (entity, item, pos) in ground_items.iter() {
        let distance = player_pos.0.distance(pos.0);
        if distance <= PICKUP_RANGE {
            if closest.is_none() || distance < closest.as_ref().unwrap().2 {
                closest = Some((entity, item, distance));
            }
        }
    }
    
    if let Some((entity, item, _distance)) = closest {
        *nearby = NearbyItem {
            entity: Some(entity),
            item_type: Some(item.item_type),
            quantity: Some(item.quantity),
        };
    } else {
        *nearby = NearbyItem::default();
    }
}

/// Show pickup prompt when near an item
fn show_pickup_prompt(
    mut commands: Commands,
    nearby: Res<NearbyItem>,
    existing_prompt: Query<Entity, With<PickupPrompt>>,
    mut text_query: Query<&mut Text, With<PickupPrompt>>,
) {
    // If we have a nearby item, show/update prompt
    if let (Some(item_type), Some(quantity)) = (nearby.item_type, nearby.quantity) {
        let prompt_text = format!("Press E to pick up {}x {}", quantity, item_type.display_name());
        
        // Update existing prompt or spawn new one
        if let Ok(mut text) = text_query.single_mut() {
            **text = prompt_text;
        } else if existing_prompt.is_empty() {
            // Spawn new prompt
            commands.spawn((
                PickupPrompt,
                Text::new(prompt_text),
                TextFont {
                    font_size: 20.0,
                    ..default()
                },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Percent(25.0),
                    left: Val::Percent(50.0),
                    ..default()
                },
                // Center the text
                TextLayout::new_with_justify(Justify::Center),
            ));
        }
    } else {
        // No item nearby, despawn prompt
        for entity in existing_prompt.iter() {
            commands.entity(entity).despawn();
        }
    }
}

/// Handle E key to pick up items
fn handle_pickup_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    nearby: Res<NearbyItem>,
    mut client_query: Query<&mut MessageSender<PickupRequest>, (With<crate::GameClient>, With<Connected>)>,
) {
    if !keyboard.just_pressed(KeyCode::KeyE) {
        return;
    }
    
    // Only request pickup if there's a nearby item
    if nearby.entity.is_none() {
        return;
    };
    
    // Send pickup request to server - server will find nearest item
    if let Ok(mut sender) = client_query.single_mut() {
        let _ = sender.send::<ReliableChannel>(PickupRequest);
        info!("Requesting pickup of nearby item");
    }
}

/// Cleanup pickup UI when leaving playing state
fn cleanup_pickup_ui(
    mut commands: Commands,
    prompts: Query<Entity, With<PickupPrompt>>,
) {
    for entity in prompts.iter() {
        commands.entity(entity).despawn();
    }
}

// =============================================================================
// GROUND ITEM VISUALS
// =============================================================================

/// Marker for ground item 3D visuals
#[derive(Component)]
pub struct GroundItemVisual {
    /// The GroundItem entity this visual belongs to
    pub source_entity: Entity,
    /// Animation timer for bobbing
    pub bob_timer: f32,
    /// Base Y position
    pub base_y: f32,
}

/// Spawn 3D visuals for new ground items
fn spawn_ground_item_visuals(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    new_items: Query<(Entity, &GroundItem, &GroundItemPosition), Added<GroundItemPosition>>,
    existing_visuals: Query<&GroundItemVisual>,
) {
    for (entity, item, pos) in new_items.iter() {
        // Check if visual already exists for this entity
        let already_exists = existing_visuals.iter().any(|v| v.source_entity == entity);
        if already_exists {
            continue;
        }
        
        // Create mesh based on item type
        let mesh = create_item_mesh(&mut meshes, item.item_type);
        let material = create_item_material(&mut materials, item.item_type);
        
        // Spawn visual at item position
        commands.spawn((
            GroundItemVisual {
                source_entity: entity,
                bob_timer: rand::random::<f32>() * std::f32::consts::TAU, // Random start phase
                base_y: pos.0.y,
            },
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(pos.0),
        ));
        
        info!("Spawned visual for ground item: {}x {}", item.quantity, item.item_type.display_name());
    }
}

/// Create mesh for item type (simple shapes)
fn create_item_mesh(meshes: &mut ResMut<Assets<Mesh>>, item_type: ItemType) -> Handle<Mesh> {
    match item_type {
        // Ammo types - small boxes
        ItemType::RifleAmmo | ItemType::PistolAmmo | ItemType::SniperRounds => {
            meshes.add(Cuboid::new(0.15, 0.1, 0.3))
        }
        ItemType::ShotgunShells => {
            meshes.add(Cylinder::new(0.08, 0.25))
        }
        // Resources - larger cubes
        ItemType::Stone => {
            meshes.add(Cuboid::new(0.35, 0.3, 0.35))
        }
        ItemType::Wood => {
            meshes.add(Cuboid::new(0.2, 0.2, 0.5))
        }
        // Weapons - longer box
        ItemType::Weapon(_) => {
            meshes.add(Cuboid::new(0.55, 0.12, 0.18))
        }
    }
}

/// Create material for item type
fn create_item_material(materials: &mut ResMut<Assets<StandardMaterial>>, item_type: ItemType) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial {
        base_color: item_type.color(),
        metallic: match item_type {
            ItemType::RifleAmmo | ItemType::PistolAmmo | ItemType::SniperRounds | ItemType::ShotgunShells => 0.8,
            ItemType::Stone => 0.1,
            ItemType::Wood => 0.0,
            ItemType::Weapon(_) => 0.35,
        },
        perceptual_roughness: match item_type {
            ItemType::RifleAmmo | ItemType::PistolAmmo | ItemType::SniperRounds | ItemType::ShotgunShells => 0.3,
            ItemType::Stone => 0.7,
            ItemType::Wood => 0.8,
            ItemType::Weapon(_) => 0.45,
        },
        emissive: item_type.color().to_linear() * 0.3, // Slight glow so items are visible
        ..default()
    })
}

/// Animate ground items with bobbing and rotation
fn animate_ground_items(
    time: Res<Time>,
    mut visuals: Query<(&mut GroundItemVisual, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let bob_speed = 2.0; // Speed of bobbing
    let bob_height = 0.15; // Height of bob
    let rotation_speed = 1.5; // Rotation speed
    
    for (mut visual, mut transform) in visuals.iter_mut() {
        // Update timer
        visual.bob_timer += dt * bob_speed;
        
        // Bob up and down
        let bob_offset = visual.bob_timer.sin() * bob_height;
        transform.translation.y = visual.base_y + bob_offset + 0.3; // +0.3 to float above ground
        
        // Rotate slowly
        transform.rotate_y(dt * rotation_speed);
    }
}

/// Despawn visuals when their source entity is removed
fn despawn_ground_item_visuals(
    mut commands: Commands,
    visuals: Query<(Entity, &GroundItemVisual)>,
    ground_items: Query<Entity, With<GroundItem>>,
) {
    for (visual_entity, visual) in visuals.iter() {
        // If the source entity no longer exists, despawn the visual
        if ground_items.get(visual.source_entity).is_err() {
            commands.entity(visual_entity).despawn();
        }
    }
}

/// Cleanup all item visuals when leaving playing state
fn cleanup_item_visuals(
    mut commands: Commands,
    visuals: Query<Entity, With<GroundItemVisual>>,
) {
    for entity in visuals.iter() {
        commands.entity(entity).despawn();
    }
}
