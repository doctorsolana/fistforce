//! Item and Inventory system
//!
//! Defines item types, inventory slots, and ground items for the game.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use crate::weapons::WeaponType;

// =============================================================================
// ITEM TYPES
// =============================================================================

/// All item types in the game
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ItemType {
    // Ammo types
    #[default]
    RifleAmmo,
    ShotgunShells,
    PistolAmmo,
    SniperRounds,
    // Resources
    Stone,
    Wood,
    // Weapons (non-stackable)
    Weapon(WeaponType),
}

impl ItemType {
    /// Get the maximum stack size for this item type
    pub fn max_stack_size(&self) -> u32 {
        match self {
            // Ammo stacks to 60
            ItemType::RifleAmmo => 60,
            ItemType::ShotgunShells => 60,
            ItemType::PistolAmmo => 60,
            ItemType::SniperRounds => 60,
            // Resources stack to 100
            ItemType::Stone => 100,
            ItemType::Wood => 100,
            // Weapons are non-stackable
            ItemType::Weapon(_) => 1,
        }
    }

    /// Get display name for this item
    pub fn display_name(&self) -> &'static str {
        match self {
            ItemType::RifleAmmo => "Rifle Ammo",
            ItemType::ShotgunShells => "Shotgun Shells",
            ItemType::PistolAmmo => "Pistol Ammo",
            ItemType::SniperRounds => "Sniper Rounds",
            ItemType::Stone => "Stone",
            ItemType::Wood => "Wood",
            ItemType::Weapon(w) => match w {
                WeaponType::Unarmed => "Unarmed",
                WeaponType::Pistol => "Pistol",
                WeaponType::AssaultRifle => "Assault Rifle",
                WeaponType::Sniper => "Sniper Rifle",
                WeaponType::Shotgun => "Shotgun",
                WeaponType::SMG => "SMG",
            },
        }
    }

    /// Get color for UI/visuals
    pub fn color(&self) -> Color {
        match self {
            ItemType::RifleAmmo => Color::srgb(0.8, 0.6, 0.2),      // Brass
            ItemType::ShotgunShells => Color::srgb(0.9, 0.2, 0.2),  // Red
            ItemType::PistolAmmo => Color::srgb(0.7, 0.7, 0.3),     // Yellow brass
            ItemType::SniperRounds => Color::srgb(0.2, 0.6, 0.2),   // Green tip
            ItemType::Stone => Color::srgb(0.5, 0.5, 0.5),          // Gray
            ItemType::Wood => Color::srgb(0.6, 0.4, 0.2),           // Brown
            ItemType::Weapon(w) => match w {
                WeaponType::Unarmed => Color::srgb(0.6, 0.6, 0.6),
                WeaponType::Pistol => Color::srgb(0.55, 0.55, 0.6),
                WeaponType::AssaultRifle => Color::srgb(0.25, 0.75, 0.35),
                WeaponType::SMG => Color::srgb(0.35, 0.6, 0.75),
                WeaponType::Shotgun => Color::srgb(0.8, 0.55, 0.25),
                WeaponType::Sniper => Color::srgb(0.75, 0.25, 0.55),
            },
        }
    }
    
    /// If this item is a weapon, return its WeaponType
    pub fn as_weapon_type(&self) -> Option<WeaponType> {
        match self {
            ItemType::Weapon(w) => Some(*w),
            _ => None,
        }
    }
}

// =============================================================================
// ITEM STACK
// =============================================================================

/// A stack of items (type + quantity)
/// For weapon items, also tracks the ammo currently loaded in the magazine.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ItemStack {
    pub item_type: ItemType,
    pub quantity: u32,
    /// For weapon items only: ammo currently in the magazine.
    /// None means the weapon has a full magazine (or this isn't a weapon).
    pub ammo_in_mag: Option<u32>,
}

impl ItemStack {
    pub fn new(item_type: ItemType, quantity: u32) -> Self {
        Self { item_type, quantity, ammo_in_mag: None }
    }
    
    /// Create a weapon item with specified magazine ammo
    pub fn new_weapon(weapon_type: WeaponType, ammo_in_mag: u32) -> Self {
        Self {
            item_type: ItemType::Weapon(weapon_type),
            quantity: 1,
            ammo_in_mag: Some(ammo_in_mag),
        }
    }
    
    /// Create a weapon item with a full magazine
    pub fn new_weapon_full_mag(weapon_type: WeaponType) -> Self {
        let mag_size = weapon_type.stats().magazine_size;
        Self::new_weapon(weapon_type, mag_size)
    }
    
    /// Get the ammo in mag for a weapon (returns full mag size if not set)
    pub fn get_weapon_ammo(&self) -> u32 {
        if let ItemType::Weapon(w) = self.item_type {
            self.ammo_in_mag.unwrap_or_else(|| w.stats().magazine_size)
        } else {
            0
        }
    }
    
    /// Set the ammo in mag for a weapon
    pub fn set_weapon_ammo(&mut self, ammo: u32) {
        if self.item_type.as_weapon_type().is_some() {
            self.ammo_in_mag = Some(ammo);
        }
    }

    /// Check if this stack can accept more of the same item type
    pub fn can_add(&self, amount: u32) -> bool {
        self.quantity + amount <= self.item_type.max_stack_size()
    }

    /// How much more can this stack hold?
    pub fn space_remaining(&self) -> u32 {
        self.item_type.max_stack_size().saturating_sub(self.quantity)
    }

    /// Add to this stack, returns amount that couldn't fit
    pub fn add(&mut self, amount: u32) -> u32 {
        let can_add = self.space_remaining().min(amount);
        self.quantity += can_add;
        amount - can_add
    }

    /// Remove from this stack, returns amount actually removed
    pub fn remove(&mut self, amount: u32) -> u32 {
        let can_remove = self.quantity.min(amount);
        self.quantity -= can_remove;
        can_remove
    }
}

// =============================================================================
// INVENTORY
// =============================================================================

/// Number of inventory slots
pub const INVENTORY_SLOTS: usize = 24;

/// Number of hotbar (equippable) slots at the top of inventory
pub const HOTBAR_SLOTS: usize = 6;

/// Player inventory with fixed slots
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Inventory {
    slots: [Option<ItemStack>; INVENTORY_SLOTS],
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            slots: [None; INVENTORY_SLOTS],
        }
    }
}

impl Inventory {
    /// Create a new empty inventory
    pub fn new() -> Self {
        Self::default()
    }

    /// Create inventory with starting items for a new player
    pub fn with_starting_items() -> Self {
        let mut inv = Self::new();
        
        // Starting weapon in hotbar slot 0 (with full magazine)
        let _ = inv.set_slot(0, Some(ItemStack::new_weapon_full_mag(WeaponType::AssaultRifle)));
        
        // Starting ammo
        inv.add_item(ItemType::RifleAmmo, 90);   // 3 stacks
        inv.add_item(ItemType::ShotgunShells, 20);
        inv.add_item(ItemType::PistolAmmo, 24);
        inv.add_item(ItemType::SniperRounds, 10);
        inv
    }

    /// Get a slot by index
    pub fn get_slot(&self, index: usize) -> Option<&ItemStack> {
        self.slots.get(index).and_then(|s| s.as_ref())
    }
    
    /// Get a mutable slot by index
    pub fn get_slot_mut(&mut self, index: usize) -> Option<&mut ItemStack> {
        self.slots.get_mut(index).and_then(|s| s.as_mut())
    }

    /// Get all slots
    pub fn slots(&self) -> &[Option<ItemStack>; INVENTORY_SLOTS] {
        &self.slots
    }

    /// Find first slot with the given item type that has space
    pub fn find_slot_with_space(&self, item_type: ItemType) -> Option<usize> {
        self.slots.iter().position(|slot| {
            if let Some(stack) = slot {
                stack.item_type == item_type && stack.space_remaining() > 0
            } else {
                false
            }
        })
    }

    /// Find first empty slot
    pub fn find_empty_slot(&self) -> Option<usize> {
        self.slots.iter().position(|slot| slot.is_none())
    }

    /// Add items to inventory, returns amount that couldn't fit
    pub fn add_item(&mut self, item_type: ItemType, mut quantity: u32) -> u32 {
        // First, try to stack with existing items
        while quantity > 0 {
            if let Some(slot_idx) = self.find_slot_with_space(item_type) {
                if let Some(stack) = &mut self.slots[slot_idx] {
                    quantity = stack.add(quantity);
                }
            } else {
                break;
            }
        }

        // Then, use empty slots
        while quantity > 0 {
            if let Some(slot_idx) = self.find_empty_slot() {
                let stack_amount = quantity.min(item_type.max_stack_size());
                self.slots[slot_idx] = Some(ItemStack::new(item_type, stack_amount));
                quantity -= stack_amount;
            } else {
                break;
            }
        }

        quantity // Return what couldn't fit
    }
    
    /// Add an ItemStack to inventory (preserves weapon ammo state).
    /// For non-stackable items (like weapons), finds an empty slot.
    /// Returns the stack if it couldn't be added, None if successful.
    pub fn add_stack(&mut self, stack: ItemStack) -> Option<ItemStack> {
        // Weapons are non-stackable (max_stack_size = 1), so just find an empty slot
        if stack.item_type.max_stack_size() == 1 {
            if let Some(slot_idx) = self.find_empty_slot() {
                self.slots[slot_idx] = Some(stack);
                return None; // Success
            }
            return Some(stack); // No room
        }
        
        // For stackable items, try to stack then use empty slots
        let mut remaining = stack;
        
        // Try to stack with existing
        while remaining.quantity > 0 {
            if let Some(slot_idx) = self.find_slot_with_space(remaining.item_type) {
                if let Some(existing) = &mut self.slots[slot_idx] {
                    remaining.quantity = existing.add(remaining.quantity);
                }
            } else {
                break;
            }
        }
        
        // Use empty slots for remainder
        while remaining.quantity > 0 {
            if let Some(slot_idx) = self.find_empty_slot() {
                let stack_amount = remaining.quantity.min(remaining.item_type.max_stack_size());
                self.slots[slot_idx] = Some(ItemStack {
                    item_type: remaining.item_type,
                    quantity: stack_amount,
                    ammo_in_mag: remaining.ammo_in_mag, // Preserve ammo for first stack
                });
                remaining.quantity -= stack_amount;
                remaining.ammo_in_mag = None; // Only first stack gets ammo
            } else {
                break;
            }
        }
        
        if remaining.quantity > 0 {
            Some(remaining)
        } else {
            None
        }
    }

    /// Remove items from inventory, returns amount actually removed
    pub fn remove_item(&mut self, item_type: ItemType, mut quantity: u32) -> u32 {
        let mut removed = 0;

        for slot in &mut self.slots {
            if quantity == 0 {
                break;
            }
            if let Some(stack) = slot {
                if stack.item_type == item_type {
                    let took = stack.remove(quantity);
                    removed += took;
                    quantity -= took;

                    // Clear slot if empty
                    if stack.quantity == 0 {
                        *slot = None;
                    }
                }
            }
        }

        removed
    }

    /// Remove item from specific slot, returns the removed stack (if any)
    pub fn remove_slot(&mut self, index: usize) -> Option<ItemStack> {
        if index < INVENTORY_SLOTS {
            self.slots[index].take()
        } else {
            None
        }
    }
    
    /// Set a slot directly (returns false if out of bounds)
    pub fn set_slot(&mut self, index: usize, stack: Option<ItemStack>) -> bool {
        if index >= INVENTORY_SLOTS {
            return false;
        }
        self.slots[index] = stack;
        true
    }
    
    /// Move an item stack between slots with Valheim-like behavior:
    /// - If target is empty: move
    /// - If same item type and stackable: stack as much as possible
    /// - Else: swap
    ///
    /// Returns true if any change was made.
    pub fn move_or_stack_slot(&mut self, from: usize, to: usize) -> bool {
        if from >= INVENTORY_SLOTS || to >= INVENTORY_SLOTS || from == to {
            return false;
        }
        
        let Some(mut from_stack) = self.slots[from].take() else {
            return false;
        };
        
        match self.slots[to].take() {
            None => {
                self.slots[to] = Some(from_stack);
                true
            }
            Some(mut to_stack) => {
                // Try stacking
                if to_stack.item_type == from_stack.item_type && to_stack.item_type.max_stack_size() > 1 {
                    let space = to_stack.space_remaining();
                    if space > 0 {
                        let transfer = space.min(from_stack.quantity);
                        to_stack.quantity += transfer;
                        from_stack.quantity -= transfer;
                        
                        self.slots[to] = Some(to_stack);
                        if from_stack.quantity == 0 {
                            self.slots[from] = None;
                        } else {
                            self.slots[from] = Some(from_stack);
                        }
                        true
                    } else {
                        // No space; put them back (no-op)
                        self.slots[to] = Some(to_stack);
                        self.slots[from] = Some(from_stack);
                        false
                    }
                } else {
                    // Swap
                    self.slots[to] = Some(from_stack);
                    self.slots[from] = Some(to_stack);
                    true
                }
            }
        }
    }

    /// Count total quantity of an item type
    pub fn count_item(&self, item_type: ItemType) -> u32 {
        self.slots
            .iter()
            .filter_map(|s| s.as_ref())
            .filter(|s| s.item_type == item_type)
            .map(|s| s.quantity)
            .sum()
    }

    /// Check if inventory has at least the specified quantity
    pub fn has_item(&self, item_type: ItemType, quantity: u32) -> bool {
        self.count_item(item_type) >= quantity
    }

    /// Check if inventory is completely empty
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.is_none())
    }

    /// Get all non-empty slots as (index, stack) pairs
    pub fn iter_items(&self) -> impl Iterator<Item = (usize, &ItemStack)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| slot.as_ref().map(|s| (i, s)))
    }
}

// =============================================================================
// GROUND ITEM
// =============================================================================

/// An item that exists in the world and can be picked up
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroundItem {
    pub item_type: ItemType,
    pub quantity: u32,
    /// For weapon items: ammo currently in the magazine (None = empty mag, needs reload)
    pub ammo_in_mag: Option<u32>,
}

impl GroundItem {
    pub fn new(item_type: ItemType, quantity: u32) -> Self {
        Self { item_type, quantity, ammo_in_mag: None }
    }
    
    /// Create a ground item from an ItemStack (preserves weapon ammo)
    pub fn from_stack(stack: &ItemStack) -> Self {
        Self {
            item_type: stack.item_type,
            quantity: stack.quantity,
            ammo_in_mag: stack.ammo_in_mag,
        }
    }
    
    /// Create a weapon ground item with specified ammo in mag
    pub fn new_weapon(weapon_type: WeaponType, ammo_in_mag: u32) -> Self {
        Self {
            item_type: ItemType::Weapon(weapon_type),
            quantity: 1,
            ammo_in_mag: Some(ammo_in_mag),
        }
    }
    
    /// Convert to an ItemStack (for adding to inventory)
    pub fn to_stack(&self) -> ItemStack {
        ItemStack {
            item_type: self.item_type,
            quantity: self.quantity,
            ammo_in_mag: self.ammo_in_mag,
        }
    }
}

/// Position component for ground items (separate from transform for networking)
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroundItemPosition(pub Vec3);

// =============================================================================
// CHEST STORAGE
// =============================================================================

/// Number of slots in a chest
pub const CHEST_SLOTS: usize = 6;

/// A chest/storage container with item slots
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChestStorage {
    pub slots: [Option<ItemStack>; CHEST_SLOTS],
}

impl Default for ChestStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl ChestStorage {
    pub fn new() -> Self {
        Self {
            slots: [None; CHEST_SLOTS],
        }
    }
    
    /// Create a chest with initial items
    pub fn with_items(items: Vec<ItemStack>) -> Self {
        let mut chest = Self::new();
        for (i, item) in items.into_iter().take(CHEST_SLOTS).enumerate() {
            chest.slots[i] = Some(item);
        }
        chest
    }
    
    /// Get a slot by index
    pub fn get_slot(&self, index: usize) -> Option<&ItemStack> {
        self.slots.get(index).and_then(|s| s.as_ref())
    }
    
    /// Get a mutable slot by index
    pub fn get_slot_mut(&mut self, index: usize) -> Option<&mut ItemStack> {
        self.slots.get_mut(index).and_then(|s| s.as_mut())
    }
    
    /// Take an item from a slot (removes it)
    pub fn take_slot(&mut self, index: usize) -> Option<ItemStack> {
        if index < CHEST_SLOTS {
            self.slots[index].take()
        } else {
            None
        }
    }
    
    /// Put an item into a slot (fails if slot occupied or out of bounds)
    pub fn put_slot(&mut self, index: usize, stack: ItemStack) -> Result<(), ItemStack> {
        if index >= CHEST_SLOTS {
            return Err(stack);
        }
        if self.slots[index].is_some() {
            return Err(stack);
        }
        self.slots[index] = Some(stack);
        Ok(())
    }
    
    /// Find first empty slot
    pub fn find_empty_slot(&self) -> Option<usize> {
        self.slots.iter().position(|slot| slot.is_none())
    }
    
    /// Check if chest is empty
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(|s| s.is_none())
    }
}

/// Position component for chests (separate from transform for networking)
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChestPosition(pub Vec3);

// =============================================================================
// MESSAGES
// =============================================================================

/// Client -> Server: Request to pick up the nearest ground item
/// The server will find the closest item within pickup range
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PickupRequest;

/// Client -> Server: Request to drop an item from inventory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DropRequest {
    /// Inventory slot index to drop from
    pub slot_index: usize,
}

/// Client -> Server: Select active hotbar slot (0..HOTBAR_SLOTS-1)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectHotbarSlot {
    pub index: u8,
}

/// Client -> Server: Request to move an item stack within the inventory (server authoritative)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryMoveRequest {
    pub from: u8,
    pub to: u8,
}

/// Replicated: which hotbar slot is currently active
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HotbarSelection {
    pub index: u8,
}

impl Default for HotbarSelection {
    fn default() -> Self {
        Self { index: 0 }
    }
}

// === CHEST MESSAGES ===

/// Client -> Server: Request to open the nearest chest
/// Server will find closest chest within range and track it as open for this client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenChestRequest;

/// Client -> Server: Request to close the currently open chest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloseChestRequest;

/// Client -> Server: Request to transfer an item between player inventory and chest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChestTransferRequest {
    /// true = chest -> player inventory, false = player inventory -> chest
    pub from_chest: bool,
    /// Source slot index
    pub from_slot: u8,
    /// Destination slot index
    pub to_slot: u8,
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inventory_add_item() {
        let mut inv = Inventory::new();
        
        // Add some items
        let overflow = inv.add_item(ItemType::RifleAmmo, 50);
        assert_eq!(overflow, 0);
        assert_eq!(inv.count_item(ItemType::RifleAmmo), 50);

        // Add more that stacks
        let overflow = inv.add_item(ItemType::RifleAmmo, 20);
        assert_eq!(overflow, 10); // 50 + 20 = 70, but max is 60, so 10 goes to new slot
        assert_eq!(inv.count_item(ItemType::RifleAmmo), 70);
    }

    #[test]
    fn test_inventory_remove_item() {
        let mut inv = Inventory::new();
        inv.add_item(ItemType::Stone, 50);

        let removed = inv.remove_item(ItemType::Stone, 30);
        assert_eq!(removed, 30);
        assert_eq!(inv.count_item(ItemType::Stone), 20);

        // Try to remove more than exists
        let removed = inv.remove_item(ItemType::Stone, 100);
        assert_eq!(removed, 20);
        assert_eq!(inv.count_item(ItemType::Stone), 0);
    }

    #[test]
    fn test_starting_items() {
        let inv = Inventory::with_starting_items();
        assert_eq!(inv.count_item(ItemType::RifleAmmo), 90);
        assert_eq!(inv.count_item(ItemType::ShotgunShells), 20);
        assert_eq!(inv.count_item(ItemType::PistolAmmo), 24);
        assert_eq!(inv.count_item(ItemType::SniperRounds), 10);
    }
}
