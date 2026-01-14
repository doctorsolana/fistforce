//! Server-side inventory management
//!
//! Handles item pickups, drops, and death drops.

use bevy::prelude::*;
use lightyear::prelude::*;
use lightyear::prelude::server::*;
use shared::{
    GroundItem, GroundItemPosition, Inventory, ItemType, ItemStack,
    PickupRequest, DropRequest,
    InventoryMoveRequest, SelectHotbarSlot, HotbarSelection,
    INVENTORY_SLOTS, HOTBAR_SLOTS, CHEST_SLOTS,
    Player, PlayerPosition, Health,
    WorldTerrain,
    EquippedWeapon, WeaponType,
    ChestStorage, ChestPosition,
    OpenChestRequest, CloseChestRequest, ChestTransferRequest,
};
use std::collections::HashMap;

/// Distance within which a player can pick up an item
const PICKUP_RANGE: f32 = 3.0;

// =============================================================================
// HOTBAR / EQUIPMENT
// =============================================================================

/// Handle hotbar selection requests from clients
pub fn handle_hotbar_selection_requests(
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<SelectHotbarSlot>), With<ClientOf>>,
    mut players: Query<(&Player, &mut HotbarSelection)>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        
        for request in receiver.receive() {
            let clamped = (request.index as usize).min(HOTBAR_SLOTS.saturating_sub(1)) as u8;
            
            if let Some((_, mut selection)) = players.iter_mut().find(|(p, _)| p.client_id == peer_id) {
                selection.index = clamped;
            }
        }
    }
}

/// Handle inventory move requests (drag & drop) from clients
pub fn handle_inventory_move_requests(
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<InventoryMoveRequest>), With<ClientOf>>,
    mut players: Query<(&Player, &mut Inventory)>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        
        for request in receiver.receive() {
            let from = request.from as usize;
            let to = request.to as usize;
            
            if let Some((_, mut inventory)) = players.iter_mut().find(|(p, _)| p.client_id == peer_id) {
                let _ = inventory.move_or_stack_slot(from, to);
            }
        }
    }
}

/// Server-authoritative: ensure `EquippedWeapon` matches the active hotbar slot.
/// If the active slot does not contain a weapon item, the player is `Unarmed`.
/// Also syncs ammo_in_mag between EquippedWeapon and the inventory slot.
pub fn sync_equipped_weapon_from_hotbar(
    mut players: Query<(&mut Inventory, &HotbarSelection, &mut EquippedWeapon, &mut PreviousHotbarSlot)>,
) {
    for (mut inventory, selection, mut equipped, mut prev_slot) in players.iter_mut() {
        let slot_idx = (selection.index as usize)
            .min(HOTBAR_SLOTS.saturating_sub(1))
            .min(INVENTORY_SLOTS.saturating_sub(1));
        
        // Get the desired weapon type from the current hotbar slot
        let (desired, slot_ammo) = inventory
            .get_slot(slot_idx)
            .map(|stack| {
                let wt = stack.item_type.as_weapon_type().unwrap_or(WeaponType::Unarmed);
                let ammo = stack.get_weapon_ammo();
                (wt, ammo)
            })
            .unwrap_or((WeaponType::Unarmed, 0));
        
        // Check if we're switching slots or weapons
        let switching = prev_slot.index != Some(slot_idx) || equipped.weapon_type != desired;
        
        if switching {
            // Save current ammo back to the previous slot (if it was a weapon)
            if let Some(prev_idx) = prev_slot.index {
                if equipped.weapon_type != WeaponType::Unarmed {
                    if let Some(stack) = inventory.get_slot_mut(prev_idx) {
                        if stack.item_type.as_weapon_type() == Some(equipped.weapon_type) {
                            stack.set_weapon_ammo(equipped.ammo_in_mag);
                        }
                    }
                }
            }
            
            // Update to the new weapon
            equipped.weapon_type = desired;
            equipped.aiming = false;
            equipped.last_fire_time = -10.0;
            equipped.reserve_ammo = 0;
            
            // Load ammo from the new slot
            if desired != WeaponType::Unarmed {
                equipped.ammo_in_mag = slot_ammo;
            } else {
                equipped.ammo_in_mag = 0;
            }
            
            prev_slot.index = Some(slot_idx);
        } else {
            // Not switching - continuously sync ammo from EquippedWeapon back to inventory
            // (so if player shoots, the inventory slot stays up to date)
            if equipped.weapon_type != WeaponType::Unarmed {
                if let Some(stack) = inventory.get_slot_mut(slot_idx) {
                    if stack.item_type.as_weapon_type() == Some(equipped.weapon_type) {
                        stack.set_weapon_ammo(equipped.ammo_in_mag);
                    }
                }
            }
        }
    }
}

/// Tracks which hotbar slot was previously active (for ammo save/load)
#[derive(Component, Default, Clone)]
pub struct PreviousHotbarSlot {
    pub index: Option<usize>,
}

/// Handle pickup requests from clients
pub fn handle_pickup_requests(
    mut commands: Commands,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<PickupRequest>), With<ClientOf>>,
    mut players: Query<(&Player, &PlayerPosition, &mut Inventory)>,
    ground_items: Query<(Entity, &GroundItem, &GroundItemPosition)>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        
        for _request in receiver.receive() {
            // Find the player for this client
            let Some((_, player_pos, mut inventory)) = players.iter_mut().find(|(p, _, _)| p.client_id == peer_id) else {
                warn!("Pickup request from unknown player {:?}", peer_id);
                continue;
            };
            
            // Find the closest ground item within pickup range
            let mut closest: Option<(Entity, &GroundItem, f32)> = None;
            for (entity, item, pos) in ground_items.iter() {
                let distance = player_pos.0.distance(pos.0);
                if distance <= PICKUP_RANGE {
                    if closest.is_none() || distance < closest.as_ref().unwrap().2 {
                        closest = Some((entity, item, distance));
                    }
                }
            }
            
            let Some((item_entity, ground_item, _distance)) = closest else {
                // No item in range
                continue;
            };
            
            // Convert ground item to stack (preserves weapon ammo)
            let stack = ground_item.to_stack();
            
            // Try to add to inventory (preserves ammo_in_mag for weapons)
            if inventory.add_stack(stack).is_none() {
                // Successfully added
                info!("Player {:?} picked up {}x {} (mag: {:?})", 
                    peer_id, ground_item.quantity, ground_item.item_type.display_name(), ground_item.ammo_in_mag);
                commands.entity(item_entity).despawn();
            } else {
                info!("Player {:?} inventory full, couldn't pick up {}", peer_id, ground_item.item_type.display_name());
            }
        }
    }
}

/// Handle drop requests from clients
pub fn handle_drop_requests(
    mut commands: Commands,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<DropRequest>), With<ClientOf>>,
    mut players: Query<(&Player, &PlayerPosition, &mut Inventory)>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        
        for request in receiver.receive() {
            // Find the player for this client
            let Some((_, player_pos, mut inventory)) = players.iter_mut().find(|(p, _, _)| p.client_id == peer_id) else {
                continue;
            };
            
            // Remove item from slot
            if let Some(mut stack) = inventory.remove_slot(request.slot_index) {
                // If it's a weapon with ammo in mag, return the ammo to inventory
                if let Some(weapon_type) = stack.item_type.as_weapon_type() {
                    let ammo_in_mag = stack.get_weapon_ammo();
                    if ammo_in_mag > 0 {
                        let ammo_type = weapon_type.ammo_type();
                        inventory.add_item(ammo_type, ammo_in_mag);
                        info!("Player {:?} returned {} {} to inventory from dropped weapon", 
                            peer_id, ammo_in_mag, ammo_type.display_name());
                    }
                    // Weapon drops with empty mag
                    stack.set_weapon_ammo(0);
                }
                
                info!("Player {:?} dropped {}x {} from slot {}", 
                    peer_id, stack.quantity, stack.item_type.display_name(), request.slot_index);
                
                // Spawn ground item slightly in front of player
                let drop_offset = Vec3::new(0.0, 0.0, 1.5); // In front of player
                let drop_pos = player_pos.0 + drop_offset;
                
                spawn_ground_item_from_stack(&mut commands, &stack, drop_pos);
            }
        }
    }
}

/// Drop all inventory items when a player dies
pub fn drop_inventory_on_death(
    mut commands: Commands,
    mut players: Query<(&Player, &PlayerPosition, &mut Inventory, &Health), Changed<Health>>,
) {
    for (player, position, mut inventory, health) in players.iter_mut() {
        if health.is_dead() && !inventory.is_empty() {
            info!("Player {:?} died, dropping inventory", player.client_id);
            
            // Collect items to drop (we need to iterate twice - once for ammo extraction, once for dropping)
            let mut items_to_drop: Vec<(shared::ItemStack, Vec3)> = Vec::new();
            let mut extra_ammo: Vec<(ItemType, u32)> = Vec::new();
            let mut drop_offset = 0.0_f32;
            
            for (_, stack) in inventory.iter_items() {
                // If it's a weapon with ammo in mag, extract the ammo as separate item
                if let Some(weapon_type) = stack.item_type.as_weapon_type() {
                    let ammo_in_mag = stack.get_weapon_ammo();
                    if ammo_in_mag > 0 {
                        extra_ammo.push((weapon_type.ammo_type(), ammo_in_mag));
                    }
                    // Weapon drops with empty mag
                    let mut weapon_stack = *stack;
                    weapon_stack.set_weapon_ammo(0);
                    
                    let offset = Vec3::new(
                        drop_offset.sin() * 1.5,
                        0.5,
                        drop_offset.cos() * 1.5,
                    );
                    items_to_drop.push((weapon_stack, position.0 + offset));
                } else {
                    // Non-weapon items drop as-is
                    let offset = Vec3::new(
                        drop_offset.sin() * 1.5,
                        0.5,
                        drop_offset.cos() * 1.5,
                    );
                    items_to_drop.push((*stack, position.0 + offset));
                }
                drop_offset += 1.2;
            }
            
            // Drop all the items
            for (stack, pos) in items_to_drop {
                spawn_ground_item_from_stack(&mut commands, &stack, pos);
            }
            
            // Drop extracted ammo as separate items
            for (ammo_type, quantity) in extra_ammo {
                let offset = Vec3::new(
                    drop_offset.sin() * 1.5,
                    0.5,
                    drop_offset.cos() * 1.5,
                );
                spawn_ground_item(&mut commands, ammo_type, quantity, position.0 + offset);
                drop_offset += 1.2;
            }
            
            // Clear inventory
            *inventory = Inventory::new();
        }
    }
}

/// Restore inventory when player respawns
pub fn restore_inventory_on_respawn(
    mut players: Query<(&Player, &mut Inventory, &Health), Changed<Health>>,
) {
    for (player, mut inventory, health) in players.iter_mut() {
        // Check if player just respawned (health went from 0 to full)
        if !health.is_dead() && inventory.is_empty() {
            info!("Player {:?} respawned, restoring starting inventory", player.client_id);
            *inventory = Inventory::with_starting_items();
        }
    }
}

/// Spawn a ground item in the world
pub fn spawn_ground_item(
    commands: &mut Commands,
    item_type: ItemType,
    quantity: u32,
    position: Vec3,
) -> Entity {
    commands.spawn((
        GroundItem::new(item_type, quantity),
        GroundItemPosition(position),
        Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
    )).id()
}

/// Spawn a ground item from an ItemStack (preserves weapon ammo state)
pub fn spawn_ground_item_from_stack(
    commands: &mut Commands,
    stack: &shared::ItemStack,
    position: Vec3,
) -> Entity {
    commands.spawn((
        GroundItem::from_stack(stack),
        GroundItemPosition(position),
        Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
    )).id()
}

/// Spawn test items near spawn point
pub fn spawn_test_items(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    spawned: Option<Res<TestItemsSpawned>>,
) {
    if spawned.is_some() {
        return;
    }
    
    commands.insert_resource(TestItemsSpawned);
    
    // Spawn some test items near spawn
    let test_items = [
        (ItemType::RifleAmmo, 30, Vec3::new(3.0, 0.0, 3.0)),
        (ItemType::ShotgunShells, 15, Vec3::new(4.0, 0.0, 3.0)),
        (ItemType::Stone, 25, Vec3::new(5.0, 0.0, 3.0)),
        (ItemType::Wood, 50, Vec3::new(6.0, 0.0, 3.0)),
    ];
    
    for (item_type, quantity, offset) in test_items {
        let ground_y = terrain.get_height(offset.x, offset.z);
        let pos = Vec3::new(offset.x, ground_y + 0.5, offset.z);
        spawn_ground_item(&mut commands, item_type, quantity, pos);
        info!("Spawned test item: {}x {} at {:?}", quantity, item_type.display_name(), pos);
    }
    
    // Spawn a test weapon (shotgun) with empty magazine - player must reload after pickup
    let weapon_offset = Vec3::new(7.0, 0.0, 3.0);
    let ground_y = terrain.get_height(weapon_offset.x, weapon_offset.z);
    let weapon_pos = Vec3::new(weapon_offset.x, ground_y + 0.5, weapon_offset.z);
    commands.spawn((
        GroundItem::new_weapon(WeaponType::Shotgun, 0), // Empty magazine!
        GroundItemPosition(weapon_pos),
        Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
    ));
    info!("Spawned test weapon: Shotgun (empty mag) at {:?}", weapon_pos);
    
    // Spawn a test chest with some weapons and ammo
    let chest_offset = Vec3::new(3.0, 0.0, 5.0);
    let chest_y = terrain.get_height(chest_offset.x, chest_offset.z);
    let chest_pos = Vec3::new(chest_offset.x, chest_y + 0.5, chest_offset.z);
    spawn_chest(&mut commands, chest_pos, vec![
        ItemStack::new_weapon(WeaponType::Sniper, 5), // Sniper with 5 rounds loaded
        ItemStack::new(ItemType::SniperRounds, 20),
        ItemStack::new(ItemType::RifleAmmo, 60),
    ]);
    info!("Spawned test chest at {:?} with Sniper, ammo", chest_pos);
}

/// Marker resource to track if test items have been spawned
#[derive(Resource)]
pub struct TestItemsSpawned;

// =============================================================================
// CHEST / STORAGE
// =============================================================================

/// Distance within which a player can interact with a chest
const CHEST_RANGE: f32 = 3.5;

/// Tracks which player has which chest open (PeerId -> chest entity)
#[derive(Resource, Default)]
pub struct OpenChests {
    pub map: HashMap<PeerId, Entity>,
}

/// Spawn a chest in the world with initial items
pub fn spawn_chest(
    commands: &mut Commands,
    position: Vec3,
    items: Vec<ItemStack>,
) -> Entity {
    commands.spawn((
        ChestStorage::with_items(items),
        ChestPosition(position),
        Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
    )).id()
}

/// Handle open chest requests from clients
pub fn handle_open_chest_requests(
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<OpenChestRequest>), With<ClientOf>>,
    players: Query<(&Player, &PlayerPosition)>,
    chests: Query<(Entity, &ChestPosition)>,
    mut open_chests: ResMut<OpenChests>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        
        for _request in receiver.receive() {
            // Find player position
            let Some((_, player_pos)) = players.iter().find(|(p, _)| p.client_id == peer_id) else {
                continue;
            };
            
            // Find the closest chest within range
            let mut closest: Option<(Entity, f32)> = None;
            for (chest_entity, chest_pos) in chests.iter() {
                let distance = player_pos.0.distance(chest_pos.0);
                if distance <= CHEST_RANGE {
                    if closest.is_none() || distance < closest.unwrap().1 {
                        closest = Some((chest_entity, distance));
                    }
                }
            }
            
            if let Some((chest_entity, _)) = closest {
                // Track this chest as open for this player
                open_chests.map.insert(peer_id, chest_entity);
                info!("Player {:?} opened chest {:?}", peer_id, chest_entity);
            }
        }
    }
}

/// Handle close chest requests from clients
pub fn handle_close_chest_requests(
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<CloseChestRequest>), With<ClientOf>>,
    mut open_chests: ResMut<OpenChests>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        
        for _request in receiver.receive() {
            if open_chests.map.remove(&peer_id).is_some() {
                info!("Player {:?} closed chest", peer_id);
            }
        }
    }
}

/// Handle chest transfer requests (move items between player inventory and chest)
pub fn handle_chest_transfer_requests(
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<ChestTransferRequest>), With<ClientOf>>,
    mut players: Query<(&Player, &mut Inventory)>,
    mut chests: Query<&mut ChestStorage>,
    open_chests: Res<OpenChests>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        
        for request in receiver.receive() {
            // Check if player has a chest open
            let Some(&chest_entity) = open_chests.map.get(&peer_id) else {
                continue;
            };
            
            // Get player inventory
            let Some((_, mut inventory)) = players.iter_mut().find(|(p, _)| p.client_id == peer_id) else {
                continue;
            };
            
            // Get chest storage
            let Ok(mut chest) = chests.get_mut(chest_entity) else {
                continue;
            };
            
            let from_slot = request.from_slot as usize;
            let to_slot = request.to_slot as usize;
            
            if request.from_chest {
                // Chest -> Player inventory
                if from_slot >= CHEST_SLOTS || to_slot >= INVENTORY_SLOTS {
                    continue;
                }
                
                // Take from chest
                if let Some(stack) = chest.take_slot(from_slot) {
                    // Try to put in player inventory slot
                    if let Some(existing) = inventory.get_slot(to_slot).cloned() {
                        // Slot occupied - try to stack or swap
                        if existing.item_type == stack.item_type && stack.item_type.max_stack_size() > 1 {
                            // Same stackable type - merge
                            if let Some(inv_stack) = inventory.get_slot_mut(to_slot) {
                                let space = inv_stack.item_type.max_stack_size() - inv_stack.quantity;
                                let transfer = space.min(stack.quantity);
                                inv_stack.quantity += transfer;
                                if transfer < stack.quantity {
                                    // Put remainder back in chest
                                    let mut remainder = stack;
                                    remainder.quantity -= transfer;
                                    let _ = chest.put_slot(from_slot, remainder);
                                }
                            }
                        } else {
                            // Swap
                            inventory.set_slot(to_slot, Some(stack));
                            let _ = chest.put_slot(from_slot, existing);
                        }
                    } else {
                        // Empty slot - just place
                        inventory.set_slot(to_slot, Some(stack));
                    }
                    info!("Player {:?} transferred item from chest slot {} to inventory slot {}", peer_id, from_slot, to_slot);
                }
            } else {
                // Player inventory -> Chest
                if from_slot >= INVENTORY_SLOTS || to_slot >= CHEST_SLOTS {
                    continue;
                }
                
                // Take from inventory
                if let Some(stack) = inventory.remove_slot(from_slot) {
                    // Try to put in chest slot
                    if let Some(existing) = chest.get_slot(to_slot).cloned() {
                        // Slot occupied - try to stack or swap
                        if existing.item_type == stack.item_type && stack.item_type.max_stack_size() > 1 {
                            // Same stackable type - merge
                            if let Some(chest_stack) = chest.get_slot_mut(to_slot) {
                                let space = chest_stack.item_type.max_stack_size() - chest_stack.quantity;
                                let transfer = space.min(stack.quantity);
                                chest_stack.quantity += transfer;
                                if transfer < stack.quantity {
                                    // Put remainder back in inventory
                                    let mut remainder = stack;
                                    remainder.quantity -= transfer;
                                    inventory.set_slot(from_slot, Some(remainder));
                                }
                            }
                        } else {
                            // Swap
                            let _ = chest.slots[to_slot] = Some(stack);
                            inventory.set_slot(from_slot, Some(existing));
                        }
                    } else {
                        // Empty slot - just place
                        let _ = chest.put_slot(to_slot, stack);
                    }
                    info!("Player {:?} transferred item from inventory slot {} to chest slot {}", peer_id, from_slot, to_slot);
                }
            }
        }
    }
}

/// Auto-close chests when players walk too far away
pub fn auto_close_distant_chests(
    players: Query<(&Player, &PlayerPosition)>,
    chests: Query<&ChestPosition>,
    mut open_chests: ResMut<OpenChests>,
) {
    // Collect players to close (can't modify while iterating)
    let mut to_close = Vec::new();
    
    for (&client_id, &chest_entity) in open_chests.map.iter() {
        // Find player position
        let Some((_, player_pos)) = players.iter().find(|(p, _)| p.client_id == client_id) else {
            // Player not found - close chest
            to_close.push(client_id);
            continue;
        };
        
        // Check chest position
        let Ok(chest_pos) = chests.get(chest_entity) else {
            // Chest not found - close
            to_close.push(client_id);
            continue;
        };
        
        // Check distance
        if player_pos.0.distance(chest_pos.0) > CHEST_RANGE + 1.0 {
            to_close.push(client_id);
            info!("Auto-closing chest for player {:?} (walked away)", client_id);
        }
    }
    
    for client_id in to_close {
        open_chests.map.remove(&client_id);
    }
}
