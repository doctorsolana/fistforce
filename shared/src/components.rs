//! Shared ECS components used by both server and client

use bevy::prelude::*;
use lightyear::prelude::PeerId;
use serde::{Deserialize, Serialize};

use crate::weapons::WeaponType;

// =============================================================================
// WORLD TIME / DAY-NIGHT CYCLE
// =============================================================================

/// Server-authoritative day/night clock replicated to clients.
///
/// The server advances `seconds_in_cycle` every fixed tick and clients use it for lighting.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct WorldTime {
    /// Current time within the full day+night cycle.
    pub seconds_in_cycle: f32,
    /// Duration of the "day" portion in seconds (includes sunrise + sunset).
    pub day_duration: f32,
    /// Duration of the "night" portion in seconds.
    pub night_duration: f32,
}

impl WorldTime {
    /// 20 minutes of daylight.
    pub const DEFAULT_DAY_DURATION: f32 = 20.0 * 60.0;
    /// 7 minutes of night (shorter nights).
    pub const DEFAULT_NIGHT_DURATION: f32 = 7.0 * 60.0;
    /// Start early morning (near sunrise).
    pub const DEFAULT_START_SECONDS_IN_DAY: f32 = 30.0;

    pub fn new(day_duration: f32, night_duration: f32, seconds_in_cycle: f32) -> Self {
        let mut wt = Self {
            seconds_in_cycle,
            day_duration,
            night_duration,
        };
        wt.wrap();
        wt
    }

    pub fn new_default() -> Self {
        Self::new(
            Self::DEFAULT_DAY_DURATION,
            Self::DEFAULT_NIGHT_DURATION,
            Self::DEFAULT_START_SECONDS_IN_DAY,
        )
    }

    pub fn cycle_duration(&self) -> f32 {
        self.day_duration + self.night_duration
    }

    pub fn is_day(&self) -> bool {
        self.seconds_in_cycle < self.day_duration
    }

    pub fn day_t(&self) -> f32 {
        if self.day_duration <= 0.0 {
            return 0.0;
        }
        (self.seconds_in_cycle / self.day_duration).clamp(0.0, 1.0)
    }

    pub fn night_t(&self) -> f32 {
        if self.night_duration <= 0.0 {
            return 0.0;
        }
        ((self.seconds_in_cycle - self.day_duration) / self.night_duration).clamp(0.0, 1.0)
    }

    pub fn advance(&mut self, dt: f32) {
        self.seconds_in_cycle += dt.max(0.0);
        self.wrap();
    }

    fn wrap(&mut self) {
        let cycle = self.cycle_duration();
        if cycle > 0.0 {
            self.seconds_in_cycle = self.seconds_in_cycle.rem_euclid(cycle);
        } else {
            self.seconds_in_cycle = 0.0;
        }
    }

    /// Returns normalized time 0.0-1.0 where:
    /// - 0.0 = midnight
    /// - 0.25 = sunrise (start of day)
    /// - 0.5 = noon (middle of day)
    /// - 0.75 = sunset (end of day, start of night)
    /// - 1.0 = back to midnight
    ///
    /// Our internal representation has day first (0 to day_duration) then night.
    /// This maps it to a more intuitive 24-hour cycle.
    pub fn normalized_time(&self) -> f32 {
        let cycle = self.cycle_duration();
        if cycle <= 0.0 {
            return 0.5; // Default to noon if misconfigured
        }

        // Fraction of day portion (sunrise to sunset)
        let day_fraction = self.day_duration / cycle; // e.g., 20/27 ≈ 0.74
        // Fraction of night portion
        let night_fraction = self.night_duration / cycle; // e.g., 7/27 ≈ 0.26

        // Current position in cycle (0 to 1)
        let cycle_pos = self.seconds_in_cycle / cycle;

        if cycle_pos < day_fraction {
            // We're in the day portion (0 to day_duration maps to sunrise->sunset = 0.25 to 0.75)
            let day_progress = cycle_pos / day_fraction; // 0 to 1 within day
            0.25 + day_progress * 0.5 // Maps to 0.25 to 0.75
        } else {
            // We're in the night portion (day_duration to cycle_end maps to sunset->sunrise = 0.75 to 1.25, wrapped)
            let night_progress = (cycle_pos - day_fraction) / night_fraction; // 0 to 1 within night
            // First half of night: 0.75 to 1.0 (evening to midnight)
            // Second half of night: 0.0 to 0.25 (midnight to sunrise)
            let night_time = 0.75 + night_progress * 0.5;
            if night_time >= 1.0 {
                night_time - 1.0
            } else {
                night_time
            }
        }
    }
}

/// Marker component for player entities
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Player {
    pub client_id: PeerId,
}

// =============================================================================
// NPCs
// =============================================================================

/// Which NPC character model to use on the client.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum NpcArchetype {
    #[default]
    Barbarian,
    Ranger,
    Mage,
    Knight,
    Rogue,
    RogueHooded,
}

/// Marker component for NPC entities (server authoritative, replicated to clients)
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Npc {
    pub id: u64,
    pub archetype: NpcArchetype,
}

/// NPC position component - replicated across network
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct NpcPosition(pub Vec3);

/// NPC rotation (yaw) - replicated across network
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct NpcRotation(pub f32);

/// Player position component - replicated across network
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerPosition(pub Vec3);

/// Player rotation (yaw only for simplicity) - replicated across network
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerRotation(pub f32);

/// Player velocity (server-authoritative). Not replicated right now.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerVelocity(pub Vec3);

/// Player grounded state (server-authoritative)
/// Computed each tick based on terrain proximity and static collider contacts
#[derive(Component, Clone, Debug, Default)]
pub struct PlayerGrounded {
    /// Whether player is on terrain
    pub on_terrain: bool,
    /// Whether player is on a static collider (prop/structure)
    pub on_static: bool,
    /// Time since last grounded (for coyote time)
    pub time_since_grounded: f32,
}

impl PlayerGrounded {
    /// Small grace period for jumping after leaving ground
    pub const COYOTE_TIME: f32 = 0.1;
    
    /// Check if player can jump (grounded or within coyote time)
    pub fn can_jump(&self) -> bool {
        self.is_grounded() || self.time_since_grounded < Self::COYOTE_TIME
    }
    
    /// Check if player is grounded on anything
    pub fn is_grounded(&self) -> bool {
        self.on_terrain || self.on_static
    }
}

/// Marker for the local player (client-side only)
#[derive(Component)]
pub struct LocalPlayer;

/// Marker for ground/terrain
#[derive(Component)]
pub struct Ground;

// =============================================================================
// HEALTH & COMBAT
// =============================================================================

/// Health component for damageable entities
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Health {
    pub current: f32,
    pub max: f32,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            current: 100.0,
            max: 100.0,
        }
    }
}

impl Health {
    pub fn new(max: f32) -> Self {
        Self { current: max, max }
    }
    
    pub fn take_damage(&mut self, amount: f32) -> bool {
        self.current = (self.current - amount).max(0.0);
        self.current <= 0.0
    }
    
    pub fn heal(&mut self, amount: f32) {
        self.current = (self.current + amount).min(self.max);
    }
    
    pub fn is_dead(&self) -> bool {
        self.current <= 0.0
    }
    
    pub fn percentage(&self) -> f32 {
        self.current / self.max
    }
}

// =============================================================================
// WEAPONS
// =============================================================================

/// Currently equipped weapon
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct EquippedWeapon {
    pub weapon_type: WeaponType,
    pub ammo_in_mag: u32,
    pub reserve_ammo: u32,
    /// Time of last shot (game time in seconds)
    pub last_fire_time: f32,
    /// Whether currently aiming down sights
    pub aiming: bool,
}

impl Default for EquippedWeapon {
    fn default() -> Self {
        let weapon = WeaponType::default();
        let stats = weapon.stats();
        Self {
            weapon_type: weapon,
            ammo_in_mag: stats.magazine_size,
            reserve_ammo: 0, // Reserve ammo now comes from inventory
            last_fire_time: -10.0, // Allow immediate first shot
            aiming: false,
        }
    }
}

impl EquippedWeapon {
    pub fn new(weapon_type: WeaponType) -> Self {
        let stats = weapon_type.stats();
        Self {
            weapon_type,
            ammo_in_mag: stats.magazine_size,
            reserve_ammo: 0, // Reserve ammo now comes from inventory
            last_fire_time: -10.0,
            aiming: false,
        }
    }
    
    /// Check if weapon can fire (has ammo and cooldown passed)
    pub fn can_fire(&self, current_time: f32) -> bool {
        let cooldown = self.weapon_type.fire_cooldown();
        self.ammo_in_mag > 0 && (current_time - self.last_fire_time) >= cooldown
    }
    
    /// Fire the weapon, consuming ammo
    pub fn fire(&mut self, current_time: f32) -> bool {
        if self.can_fire(current_time) {
            self.ammo_in_mag -= 1;
            self.last_fire_time = current_time;
            true
        } else {
            false
        }
    }
    
    /// Reload from reserve ammo (deprecated - use reload_from_inventory instead)
    pub fn reload(&mut self) {
        let stats = self.weapon_type.stats();
        let needed = stats.magazine_size - self.ammo_in_mag;
        let available = needed.min(self.reserve_ammo);
        self.ammo_in_mag += available;
        self.reserve_ammo -= available;
    }
    
    /// Reload from inventory, returns amount of ammo consumed from inventory
    pub fn reload_from_inventory(&mut self, inventory: &mut crate::items::Inventory) -> u32 {
        let stats = self.weapon_type.stats();
        let ammo_type = self.weapon_type.ammo_type();
        let needed = stats.magazine_size - self.ammo_in_mag;
        
        if needed == 0 {
            return 0;
        }
        
        // Take ammo from inventory
        let taken = inventory.remove_item(ammo_type, needed);
        self.ammo_in_mag += taken;
        taken
    }
    
    /// Check how much reserve ammo is available in inventory
    pub fn get_reserve_from_inventory(&self, inventory: &crate::items::Inventory) -> u32 {
        inventory.count_item(self.weapon_type.ammo_type())
    }
    
    /// Get current spread based on aiming state
    pub fn current_spread(&self) -> f32 {
        let stats = self.weapon_type.stats();
        if self.aiming {
            stats.spread_ads
        } else {
            stats.spread_hip
        }
    }
}

// =============================================================================
// BULLETS
// =============================================================================

/// Bullet entity component - server authoritative, replicated to clients
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Bullet {
    /// Client ID of the shooter
    pub owner_id: u64,
    /// Weapon that fired this bullet
    pub weapon_type: WeaponType,
    /// Where the bullet was spawned (for damage falloff calculation)
    pub spawn_position: Vec3,
    /// Initial velocity at spawn (for debug visualization / deterministic re-sim)
    pub initial_velocity: Vec3,
    /// When the bullet was spawned (game time)
    pub spawn_time: f32,
}

/// Bullet velocity component - updated each physics tick
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct BulletVelocity(pub Vec3);

/// Previous position for hit detection (raycast from prev to current)
#[derive(Component, Clone, Debug, Default)]
pub struct BulletPrevPosition(pub Vec3);

/// Marker for local tracer visuals (client-side only, not replicated)
#[derive(Component)]
pub struct LocalTracer {
    pub spawn_time: f32,
    pub lifetime: f32,
}

/// Event component to signal that an NPC took damage (server-side only, not replicated)
/// This is added temporarily and consumed by the AI system
#[derive(Component, Clone, Debug)]
pub struct NpcDamageEvent {
    pub damage_source_position: Vec3,
    pub damage_amount: f32,
}
