//! World systems
//!
//! Spawning world visuals, practice wall, and other static environment.

use bevy::prelude::*;
use bevy::light::{light_consts::lux, CascadeShadowConfigBuilder, SunDisk};

use super::rendering::{SunLight, MoonLight};

// =============================================================================
// COMPONENTS
// =============================================================================

/// Root entity for all client-side world visuals
#[derive(Component)]
pub struct ClientWorldRoot;

/// Marker for the practice shooting wall
#[derive(Component)]
pub struct PracticeWall;

// =============================================================================
// SPAWNING
// =============================================================================

/// Spawn the visual world
pub fn spawn_world(
    mut commands: Commands, 
    world_roots: Query<Entity, With<ClientWorldRoot>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    terrain: Res<shared::terrain::WorldTerrain>,
) {
    if !world_roots.is_empty() {
        return;
    }

    let root = commands
        // IMPORTANT: this is the parent of terrain chunks / props / lights.
        // It must have GlobalTransform or Bevy will emit B0004 warnings for children.
        .spawn((
            ClientWorldRoot,
            Transform::default(),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
        ))
        .id();

    // --- Sun light (driven by day/night cycle) ---
    // Brighter, more intense desert sun
    let sun_light_entity = commands
        .spawn((
        SunLight,
        DirectionalLight {
            // Use unfiltered sunlight intensity; the atmosphere will handle scattering.
            illuminance: lux::RAW_SUNLIGHT,
            shadows_enabled: true,
            // Slightly warm sun color for desert environment
            color: Color::srgb(1.0, 0.98, 0.92),
            ..default()
        },
        // Performance: keep shadows enabled, but make them cheaper.
        //
        // Default is 4 cascades out to 150m. With dense/high-poly scenes, that
        // can become very expensive (shadow pass per cascade).
        CascadeShadowConfigBuilder {
            num_cascades: 3,
            maximum_distance: 120.0,
            first_cascade_far_bound: 12.0,
            ..default()
        }
        .build(),
        // Brighter sun disk for that harsh desert sun feel
        SunDisk {
            angular_size: 0.00930842,  // Same as EARTH
            intensity: 1.8,            // 80% brighter - blazing desert sun!
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.7, 0.3, 0.0)),
        ))
        .id();
    commands.entity(root).add_child(sun_light_entity);

    // --- Moon light (provides visibility at night) ---
    // Softer, cooler light from the opposite direction of the sun
    let moon_light_entity = commands
        .spawn((
            MoonLight,
            DirectionalLight {
                // Moonlight is much dimmer than sunlight (~0.3 lux vs 100,000 lux)
                // But we boost it for gameplay visibility
                illuminance: 800.0, // Boosted for gameplay - real moonlight is ~0.3 lux
                shadows_enabled: false, // No moon shadows for performance
                // Cool silvery-blue moonlight color
                color: Color::srgb(0.7, 0.8, 1.0),
                ..default()
            },
            // Moon starts on opposite side of the sky from the sun
            Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, 0.7, -0.3, 0.0)),
        ))
        .id();
    commands.entity(root).add_child(moon_light_entity);

    // Initial ambient (will be updated by day/night cycle)
    // Desert environment: warm, bright ambient
    commands.insert_resource(AmbientLight {
        color: Color::srgb(0.9, 0.85, 0.75),
        brightness: 80.0,  // Brighter for desert environment
        affects_lightmapped_meshes: true,
    });

    // Initial sky color (will be updated by day/night cycle)
    // Atmosphere handles this, but set a fallback
    commands.insert_resource(ClearColor(Color::srgb(0.85, 0.75, 0.6)));

    // =========================================================================
    // PRACTICE SHOOTING WALL - 50 meters north of spawn
    // =========================================================================
    let wall_x = 0.0;
    let wall_z = 50.0; // North
    let wall_ground_height = terrain.get_height(wall_x, wall_z);
    
    // Main wall (big concrete-looking wall)
    let wall_width = 20.0;
    let wall_height = 10.0;
    let wall_thickness = 1.0;
    
    let wall_mesh = meshes.add(Cuboid::new(wall_width, wall_height, wall_thickness));
    let wall_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.6, 0.65),
        perceptual_roughness: 0.9,
        metallic: 0.0,
        ..default()
    });
    
    let wall_entity = commands.spawn((
        Mesh3d(wall_mesh),
        MeshMaterial3d(wall_material),
        Transform::from_translation(Vec3::new(wall_x, wall_ground_height + wall_height / 2.0, wall_z)),
        PracticeWall,
    )).id();
    commands.entity(root).add_child(wall_entity);
    
    // Add some target circles on the wall
    let target_material_red = materials.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.2, 0.2),
        perceptual_roughness: 0.7,
        ..default()
    });
    let target_material_white = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.95, 0.95),
        perceptual_roughness: 0.7,
        ..default()
    });
    
    // Center target (bullseye style) - (x_offset, y_offset)
    let target_positions = [
        (0.0, 5.0),   // Center
        (0.0, 8.0),   // Top
        (0.0, 2.0),   // Bottom
        (-5.0, 5.0),  // Left
        (5.0, 5.0),   // Right
    ];
    
    for (tx, ty) in target_positions.iter() {
        let target_y = wall_ground_height + ty;
        
        // Outer ring (white) - on south-facing side of wall
        let outer_ring = meshes.add(Cylinder::new(1.0, 0.05));
        let outer_entity = commands.spawn((
            Mesh3d(outer_ring),
            MeshMaterial3d(target_material_white.clone()),
            Transform::from_translation(Vec3::new(wall_x + tx, target_y, wall_z - wall_thickness / 2.0 - 0.02))
                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        )).id();
        commands.entity(root).add_child(outer_entity);
        
        // Middle ring (red)
        let middle_ring = meshes.add(Cylinder::new(0.6, 0.06));
        let middle_entity = commands.spawn((
            Mesh3d(middle_ring),
            MeshMaterial3d(target_material_red.clone()),
            Transform::from_translation(Vec3::new(wall_x + tx, target_y, wall_z - wall_thickness / 2.0 - 0.04))
                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        )).id();
        commands.entity(root).add_child(middle_entity);
        
        // Inner ring (white)
        let inner_ring = meshes.add(Cylinder::new(0.3, 0.07));
        let inner_entity = commands.spawn((
            Mesh3d(inner_ring),
            MeshMaterial3d(target_material_white.clone()),
            Transform::from_translation(Vec3::new(wall_x + tx, target_y, wall_z - wall_thickness / 2.0 - 0.06))
                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        )).id();
        commands.entity(root).add_child(inner_entity);
        
        // Bullseye center (red)
        let bullseye = meshes.add(Cylinder::new(0.1, 0.08));
        let bullseye_entity = commands.spawn((
            Mesh3d(bullseye),
            MeshMaterial3d(target_material_red.clone()),
            Transform::from_translation(Vec3::new(wall_x + tx, target_y, wall_z - wall_thickness / 2.0 - 0.08))
                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        )).id();
        commands.entity(root).add_child(bullseye_entity);
    }
    
    // Distance markers on the ground leading north to the wall
    let marker_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.3, 0.3, 0.35),
        perceptual_roughness: 0.9,
        ..default()
    });
    
    for dist in [10, 20, 30, 40, 50] {
        let marker_z = dist as f32;
        let marker_ground = terrain.get_height(0.0, marker_z);
        let marker_mesh = meshes.add(Cuboid::new(2.0, 0.1, 0.5));
        let marker_entity = commands.spawn((
            Mesh3d(marker_mesh),
            MeshMaterial3d(marker_material.clone()),
            Transform::from_translation(Vec3::new(0.0, marker_ground + 0.05, marker_z)),
        )).id();
        commands.entity(root).add_child(marker_entity);
    }

    info!("Spawned client world visuals + practice wall 50m north");
}
