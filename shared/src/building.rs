//! Building definitions for the build mode system
//!
//! Defines building types, their resource costs, footprints, and terrain modification parameters.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::items::ItemType;

/// Types of buildings that can be constructed
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum BuildingType {
    #[default]
    TrainStation,
    CoalFactory,
    // Houses
    House01,
    House02,
    House03,
    House04,
    House09,
    House10,
}

/// All building types with GLTF models (for collider baking)
pub const ALL_BUILDING_TYPES: &[BuildingType] = &[
    BuildingType::House01,
    BuildingType::House02,
    BuildingType::House03,
    BuildingType::House04,
    BuildingType::House09,
    BuildingType::House10,
];

impl BuildingType {
    /// Get all available building types
    pub fn all() -> &'static [BuildingType] {
        &[
            BuildingType::TrainStation,
            BuildingType::CoalFactory,
            BuildingType::House01,
            BuildingType::House02,
            BuildingType::House03,
            BuildingType::House04,
            BuildingType::House09,
            BuildingType::House10,
        ]
    }

    /// Stable string id used by the collider bake manifest / database.
    pub const fn id(&self) -> &'static str {
        match self {
            BuildingType::TrainStation => "building_train_station",
            BuildingType::CoalFactory => "building_coal_factory",
            BuildingType::House01 => "building_house_01",
            BuildingType::House02 => "building_house_02",
            BuildingType::House03 => "building_house_03",
            BuildingType::House04 => "building_house_04",
            BuildingType::House09 => "building_house_09",
            BuildingType::House10 => "building_house_10",
        }
    }

    /// GLTF scene path for this building type (used by collider baker).
    /// Returns None for buildings without GLTF models.
    pub const fn scene_path(&self) -> Option<&'static str> {
        match self {
            BuildingType::TrainStation => None,
            BuildingType::CoalFactory => None,
            BuildingType::House01 => Some("buildings/House_01_full.glb#Scene0"),
            BuildingType::House02 => Some("buildings/House_02_full.glb#Scene0"),
            BuildingType::House03 => Some("buildings/House_03_full.glb#Scene0"),
            BuildingType::House04 => Some("buildings/House_04_full.glb#Scene0"),
            BuildingType::House09 => Some("buildings/House_09_full.glb#Scene0"),
            BuildingType::House10 => Some("buildings/House_10_full.glb#Scene0"),
        }
    }

    /// Check if this building type has a baked collider (GLTF model)
    pub const fn has_baked_collider(&self) -> bool {
        self.scene_path().is_some()
    }

    /// Get the definition for this building type
    pub fn definition(&self) -> BuildingDef {
        match self {
            BuildingType::TrainStation => BuildingDef {
                building_type: *self,
                display_name: "Train Station",
                cost: &[(ItemType::Wood, 20), (ItemType::Stone, 10)],
                footprint: Vec2::new(12.0, 8.0), // 12m x 8m
                height: 6.0,
                flatten_radius: 2.0, // Extra 2m around footprint for smooth transition
                color: Color::srgb(0.6, 0.5, 0.4), // Brownish
                model_path: None,
            },
            BuildingType::CoalFactory => BuildingDef {
                building_type: *self,
                display_name: "Coal Factory",
                cost: &[(ItemType::Wood, 15), (ItemType::Stone, 25)],
                footprint: Vec2::new(10.0, 10.0), // 10m x 10m
                height: 8.0,
                flatten_radius: 2.0,
                color: Color::srgb(0.3, 0.3, 0.35), // Dark gray
                model_path: None,
            },
            // Houses - small residential buildings
            BuildingType::House01 => BuildingDef {
                building_type: *self,
                display_name: "Small House",
                cost: &[(ItemType::Wood, 10), (ItemType::Stone, 5)],
                footprint: Vec2::new(6.0, 6.0),
                height: 5.0,
                flatten_radius: 1.5,
                color: Color::srgb(0.7, 0.6, 0.5),
                model_path: Some("buildings/House_01_full.glb#Scene0"),
            },
            BuildingType::House02 => BuildingDef {
                building_type: *self,
                display_name: "Cottage",
                cost: &[(ItemType::Wood, 12), (ItemType::Stone, 6)],
                footprint: Vec2::new(6.0, 7.0),
                height: 5.0,
                flatten_radius: 1.5,
                color: Color::srgb(0.6, 0.5, 0.4),
                model_path: Some("buildings/House_02_full.glb#Scene0"),
            },
            BuildingType::House03 => BuildingDef {
                building_type: *self,
                display_name: "Farmhouse",
                cost: &[(ItemType::Wood, 14), (ItemType::Stone, 8)],
                footprint: Vec2::new(7.0, 8.0),
                height: 6.0,
                flatten_radius: 1.5,
                color: Color::srgb(0.65, 0.55, 0.45),
                model_path: Some("buildings/House_03_full.glb#Scene0"),
            },
            BuildingType::House04 => BuildingDef {
                building_type: *self,
                display_name: "Village House",
                cost: &[(ItemType::Wood, 12), (ItemType::Stone, 7)],
                footprint: Vec2::new(6.0, 7.0),
                height: 5.5,
                flatten_radius: 1.5,
                color: Color::srgb(0.6, 0.5, 0.45),
                model_path: Some("buildings/House_04_full.glb#Scene0"),
            },
            BuildingType::House09 => BuildingDef {
                building_type: *self,
                display_name: "Large House",
                cost: &[(ItemType::Wood, 18), (ItemType::Stone, 12)],
                footprint: Vec2::new(8.0, 9.0),
                height: 7.0,
                flatten_radius: 2.0,
                color: Color::srgb(0.55, 0.5, 0.45),
                model_path: Some("buildings/House_09_full.glb#Scene0"),
            },
            BuildingType::House10 => BuildingDef {
                building_type: *self,
                display_name: "Manor",
                cost: &[(ItemType::Wood, 25), (ItemType::Stone, 18)],
                footprint: Vec2::new(10.0, 12.0),
                height: 8.0,
                flatten_radius: 2.5,
                color: Color::srgb(0.5, 0.45, 0.4),
                model_path: Some("buildings/House_10_full.glb#Scene0"),
            },
        }
    }

    /// Get display name
    pub fn display_name(&self) -> &'static str {
        self.definition().display_name
    }
}

/// Definition of a building's properties
#[derive(Debug, Clone)]
pub struct BuildingDef {
    pub building_type: BuildingType,
    pub display_name: &'static str,
    /// Resource cost as (ItemType, quantity) pairs
    pub cost: &'static [(ItemType, u32)],
    /// Building footprint in meters (width x depth)
    pub footprint: Vec2,
    /// Building height in meters
    pub height: f32,
    /// Extra radius around footprint for terrain flattening (smooth transition)
    pub flatten_radius: f32,
    /// Building color (for dummy mesh fallback)
    pub color: Color,
    /// Optional GLTF model path (if None, uses generated box mesh)
    pub model_path: Option<&'static str>,
}

impl BuildingDef {
    /// Get the total area that needs terrain flattening
    pub fn flatten_footprint(&self) -> Vec2 {
        Vec2::new(
            self.footprint.x + self.flatten_radius * 2.0,
            self.footprint.y + self.flatten_radius * 2.0,
        )
    }
}

/// Component for placed buildings (replicated)
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlacedBuilding {
    pub building_type: BuildingType,
    /// Rotation in radians (Y-axis rotation)
    pub rotation: f32,
}

/// Network position for placed buildings
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildingPosition(pub Vec3);

/// Message from client to request placing a building
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct PlaceBuildingRequest {
    pub building_type: BuildingType,
    pub position: Vec3,
    /// Rotation in radians (Y-axis)
    pub rotation: f32,
}

// =============================================================================
// BUILD ZONE HELPERS
// =============================================================================

/// Check if a point (XZ plane) is inside a rotated rectangle
/// 
/// * `point_xz` - Point to test (x, z world coordinates)
/// * `center_xz` - Center of the rectangle (x, z)
/// * `half_extents` - Half-width and half-depth of the rectangle
/// * `rotation_y` - Rotation around Y axis in radians
pub fn point_in_rotated_rect(
    point_xz: Vec2,
    center_xz: Vec2,
    half_extents: Vec2,
    rotation_y: f32,
) -> bool {
    // Transform point to local rect coordinates (inverse rotation)
    let rel = point_xz - center_xz;
    let cos_r = rotation_y.cos();
    let sin_r = rotation_y.sin();
    
    // Inverse rotation: rotate by -angle
    let local_x = rel.x * cos_r + rel.y * sin_r;
    let local_z = -rel.x * sin_r + rel.y * cos_r;
    
    // Check if inside axis-aligned rect in local space
    local_x.abs() <= half_extents.x && local_z.abs() <= half_extents.y
}

/// Check if a point is inside any build zone (building footprint + flatten radius)
/// Returns true if the point should be excluded (prop/collider should not spawn here)
pub fn point_in_any_build_zone(
    point_xz: Vec2,
    buildings: &[(Vec3, BuildingType, f32)], // (position, type, rotation)
) -> bool {
    for (pos, building_type, rotation) in buildings {
        let def = building_type.definition();
        let half_extents = Vec2::new(
            def.footprint.x / 2.0 + def.flatten_radius,
            def.footprint.y / 2.0 + def.flatten_radius,
        );
        let center_xz = Vec2::new(pos.x, pos.z);
        
        if point_in_rotated_rect(point_xz, center_xz, half_extents, *rotation) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_building_definitions() {
        let station = BuildingType::TrainStation.definition();
        assert_eq!(station.display_name, "Train Station");
        assert_eq!(station.cost.len(), 2);
        
        let factory = BuildingType::CoalFactory.definition();
        assert_eq!(factory.display_name, "Coal Factory");
    }

    #[test]
    fn test_flatten_footprint() {
        let def = BuildingType::TrainStation.definition();
        let flatten = def.flatten_footprint();
        // 12 + 2*2 = 16, 8 + 2*2 = 12
        assert_eq!(flatten.x, 16.0);
        assert_eq!(flatten.y, 12.0);
    }
}
