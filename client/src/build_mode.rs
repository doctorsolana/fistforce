//! Build mode system for placing buildings
//!
//! Allows players to select buildings, preview placement with terrain flattening,
//! and place buildings using resources from their inventory.

use bevy::prelude::*;
use bevy::mesh::{Indices, PrimitiveTopology};
use lightyear::prelude::*;
use std::collections::HashMap;

use shared::{
    BuildingType, PlaceBuildingRequest, PlacedBuilding, BuildingPosition, Inventory,
    WorldTerrain, LocalPlayer, WeaponDebugMode, ALL_BUILDING_TYPES,
};

use crate::input::InputState;
use crate::states::GameState;

/// Client-side building collider data for debug visualization
#[derive(Resource, Default)]
pub struct BuildingColliderVisuals {
    /// Convex hull edges for each building type (for debug drawing)
    pub hull_edges: HashMap<BuildingType, Vec<(Vec3, Vec3)>>,
}

/// Plugin for the build mode system
pub struct BuildModePlugin;

impl Plugin for BuildModePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BuildModeState>();
        app.init_resource::<BuildModeAssets>();
        app.init_resource::<BuildingColliderVisuals>();

        app.add_systems(Startup, (setup_build_mode_assets, load_building_collider_visuals));

        app.add_systems(
            Update,
            (
                toggle_build_mode,
                handle_building_selection,
                update_placement_preview,
                handle_rotation_input,
                handle_place_building,
                update_build_mode_ui,
                // Building rendering (always runs, not just in build mode)
                spawn_building_visuals,
                cleanup_building_visuals,
                // Debug visualization for building colliders
                debug_draw_building_colliders,
            )
                .run_if(in_state(GameState::Playing)),
        );

        app.add_systems(OnExit(GameState::Playing), cleanup_build_mode);
    }
}

/// Build mode state
#[derive(Resource, Default)]
pub struct BuildModeState {
    /// Whether build mode is currently active
    pub active: bool,
    /// Currently selected building type
    pub selected_building: Option<BuildingType>,
    /// Preview position in world space (where building would be placed)
    pub preview_position: Option<Vec3>,
    /// Preview rotation in radians (Y-axis)
    pub preview_rotation: f32,
    /// Whether the current position is valid for placement
    pub can_place: bool,
    /// Reason why placement is invalid (for UI feedback)
    pub invalid_reason: Option<String>,
    /// Entity for the ghost building preview mesh (footprint box with green/red)
    pub ghost_entity: Option<Entity>,
    /// Entity for the GLTF model preview (actual building model)
    pub model_preview_entity: Option<Entity>,
    /// Currently previewed building type (to detect changes)
    pub previewed_building_type: Option<BuildingType>,
    /// Entity for the terrain flattening preview
    pub terrain_preview_entity: Option<Entity>,
    /// Mesh handle for the terrain preview (updated in-place to avoid asset churn)
    pub terrain_preview_mesh: Option<Handle<Mesh>>,
    /// UI root entity
    pub ui_entity: Option<Entity>,
}

/// Pre-generated meshes and materials for build mode
#[derive(Resource, Default)]
pub struct BuildModeAssets {
    /// Ghost material (semi-transparent green for valid, red for invalid)
    pub valid_material: Handle<StandardMaterial>,
    pub invalid_material: Handle<StandardMaterial>,
    /// Terrain preview material
    pub terrain_preview_material: Handle<StandardMaterial>,
    /// Building meshes by type (fallback for buildings without models)
    pub building_meshes: std::collections::HashMap<BuildingType, Handle<Mesh>>,
    /// Building GLTF scenes by type (for buildings with model_path)
    pub building_scenes: std::collections::HashMap<BuildingType, Handle<Scene>>,
}

// UI styling constants
const UI_BG: Color = Color::srgba(0.1, 0.1, 0.12, 0.95);
const UI_BORDER: Color = Color::srgb(0.3, 0.3, 0.35);
const UI_ACCENT: Color = Color::srgb(0.4, 0.7, 0.4);
const UI_TEXT: Color = Color::srgb(0.9, 0.9, 0.9);
const UI_TEXT_DIM: Color = Color::srgb(0.6, 0.6, 0.6);
const SLOT_SELECTED: Color = Color::srgb(0.3, 0.5, 0.3);
const SLOT_NORMAL: Color = Color::srgb(0.15, 0.15, 0.18);

/// Marker for build mode UI elements
#[derive(Component)]
pub struct BuildModeUI;

/// Marker for building selection buttons
#[derive(Component)]
pub struct BuildingSelectButton {
    pub building_type: BuildingType,
}

/// Marker for the ghost preview entity (footprint box)
#[derive(Component)]
pub struct GhostPreview;

/// Marker for the model preview entity (actual GLTF model)
#[derive(Component)]
pub struct ModelPreview;

/// Marker for the terrain preview entity
#[derive(Component)]
pub struct TerrainPreview;

/// Marker for rendered building entities (visual representation of PlacedBuilding)
#[derive(Component)]
pub struct BuildingVisual {
    /// The server entity this visual represents
    pub server_entity: Entity,
}

/// Setup build mode assets at startup
fn setup_build_mode_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    let valid_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.2, 0.8, 0.2, 0.5),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });

    let invalid_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.8, 0.2, 0.2, 0.5),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    });

    let terrain_preview_material = materials.add(StandardMaterial {
        base_color: Color::srgba(0.4, 0.6, 0.4, 0.4),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        double_sided: true,
        cull_mode: None,
        ..default()
    });

    // Generate building meshes (fallback) and load GLTF scenes
    let mut building_meshes = std::collections::HashMap::new();
    let mut building_scenes = std::collections::HashMap::new();

    for building_type in BuildingType::all() {
        let def = building_type.definition();

        // Always generate fallback mesh
        let mesh = generate_building_mesh(&def);
        building_meshes.insert(*building_type, meshes.add(mesh));

        // Load GLTF scene if available
        if let Some(model_path) = def.model_path {
            let scene: Handle<Scene> = asset_server.load(model_path);
            building_scenes.insert(*building_type, scene);
        }
    }

    commands.insert_resource(BuildModeAssets {
        valid_material,
        invalid_material,
        terrain_preview_material,
        building_meshes,
        building_scenes,
    });
}

/// Load baked building colliders for debug visualization
fn load_building_collider_visuals(mut collider_visuals: ResMut<BuildingColliderVisuals>) {
    let path = "client/assets/colliders.bin";
    let Ok(db) = shared::load_baked_collider_db_from_file(path) else {
        warn!("Failed to load colliders.bin for debug visualization");
        return;
    };

    for building_type in ALL_BUILDING_TYPES.iter().copied() {
        let Some(collider) = db.entries.get(building_type.id()) else {
            continue;
        };

        match collider {
            shared::BakedCollider::ConvexHull { points } => {
                // Convert points to Vec3
                let vertices: Vec<Vec3> = points
                    .iter()
                    .map(|p| Vec3::new(p[0], p[1], p[2]))
                    .collect();

                // Extract edges from the convex hull
                // For a convex hull, we need to find which vertices are connected
                // A simple approach: compute the hull faces and extract edges
                let edges = extract_hull_edges(&vertices);
                collider_visuals.hull_edges.insert(building_type, edges);
            }
        }
    }

    info!(
        "Loaded {} building collider visuals for debug",
        collider_visuals.hull_edges.len()
    );
}

/// Extract bounding box edges from convex hull vertices
/// Returns edges for a tight axis-aligned bounding box
fn extract_hull_edges(vertices: &[Vec3]) -> Vec<(Vec3, Vec3)> {
    if vertices.len() < 4 {
        return Vec::new();
    }

    // Compute AABB from vertices
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);

    for v in vertices {
        min = min.min(*v);
        max = max.max(*v);
    }

    // Build box corners
    let corners = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(max.x, max.y, max.z),
        Vec3::new(min.x, max.y, max.z),
    ];

    // 12 edges of a box
    vec![
        // Bottom face
        (corners[0], corners[1]),
        (corners[1], corners[2]),
        (corners[2], corners[3]),
        (corners[3], corners[0]),
        // Top face
        (corners[4], corners[5]),
        (corners[5], corners[6]),
        (corners[6], corners[7]),
        (corners[7], corners[4]),
        // Verticals
        (corners[0], corners[4]),
        (corners[1], corners[5]),
        (corners[2], corners[6]),
        (corners[3], corners[7]),
    ]
}

/// Generate a simple box mesh for a building
fn generate_building_mesh(def: &shared::BuildingDef) -> Mesh {
    let half_x = def.footprint.x / 2.0;
    let half_z = def.footprint.y / 2.0;
    let height = def.height;
    
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    
    // Generate a box with proper faces
    let faces = [
        // +X face
        ([1.0, 0.0, 0.0], [
            [half_x, 0.0, -half_z],
            [half_x, height, -half_z],
            [half_x, height, half_z],
            [half_x, 0.0, half_z],
        ]),
        // -X face
        ([-1.0, 0.0, 0.0], [
            [-half_x, 0.0, half_z],
            [-half_x, height, half_z],
            [-half_x, height, -half_z],
            [-half_x, 0.0, -half_z],
        ]),
        // +Y face (top)
        ([0.0, 1.0, 0.0], [
            [-half_x, height, -half_z],
            [-half_x, height, half_z],
            [half_x, height, half_z],
            [half_x, height, -half_z],
        ]),
        // -Y face (bottom)
        ([0.0, -1.0, 0.0], [
            [-half_x, 0.0, half_z],
            [-half_x, 0.0, -half_z],
            [half_x, 0.0, -half_z],
            [half_x, 0.0, half_z],
        ]),
        // +Z face
        ([0.0, 0.0, 1.0], [
            [-half_x, 0.0, half_z],
            [half_x, 0.0, half_z],
            [half_x, height, half_z],
            [-half_x, height, half_z],
        ]),
        // -Z face
        ([0.0, 0.0, -1.0], [
            [half_x, 0.0, -half_z],
            [-half_x, 0.0, -half_z],
            [-half_x, height, -half_z],
            [half_x, height, -half_z],
        ]),
    ];
    
    for (normal, verts) in faces.iter() {
        let base_idx = positions.len() as u32;
        
        for (j, vert) in verts.iter().enumerate() {
            positions.push(*vert);
            normals.push(*normal);
            uvs.push([
                if j == 0 || j == 3 { 0.0 } else { 1.0 },
                if j == 0 || j == 1 { 0.0 } else { 1.0 },
            ]);
        }
        
        indices.push(base_idx);
        indices.push(base_idx + 1);
        indices.push(base_idx + 2);
        indices.push(base_idx);
        indices.push(base_idx + 2);
        indices.push(base_idx + 3);
    }
    
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Generate a terrain preview mesh that samples predicted post-flatten height
/// Uses the same smoothstep math as the server's apply_flatten_rect
fn generate_terrain_preview_mesh(
    terrain: &WorldTerrain,
    center: Vec3,
    half_extents: Vec2,
    rotation_y: f32,
    blend_width: f32,
) -> Mesh {
    // Low-res preview grid (2m spacing)
    const GRID_SPACING: f32 = 2.0;
    
    let total_half_x = half_extents.x + blend_width;
    let total_half_z = half_extents.y + blend_width;
    
    let steps_x = ((total_half_x * 2.0 / GRID_SPACING).ceil() as usize).max(4);
    let steps_z = ((total_half_z * 2.0 / GRID_SPACING).ceil() as usize).max(4);
    
    let cos_r = rotation_y.cos();
    let sin_r = rotation_y.sin();
    let target_height = center.y;
    
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    
    // Generate vertices
    for zi in 0..=steps_z {
        for xi in 0..=steps_x {
            // Local coordinates in rect space
            let local_x = -total_half_x + (xi as f32 / steps_x as f32) * (total_half_x * 2.0);
            let local_z = -total_half_z + (zi as f32 / steps_z as f32) * (total_half_z * 2.0);
            
            // Rotate to world space
            let world_x = center.x + local_x * cos_r - local_z * sin_r;
            let world_z = center.z + local_x * sin_r + local_z * cos_r;
            
            // Get current procedural height
            let procedural_h = terrain.get_height(world_x, world_z);
            
            // Calculate blend factor (same math as server)
            let dist_x = local_x.abs() - half_extents.x;
            let dist_z = local_z.abs() - half_extents.y;
            
            let blend_factor = if dist_x <= 0.0 && dist_z <= 0.0 {
                // Fully inside the rect
                1.0
            } else if dist_x <= blend_width && dist_z <= blend_width {
                // In the blend zone
                let edge_dist = dist_x.max(0.0).max(dist_z.max(0.0));
                if edge_dist >= blend_width {
                    0.0
                } else {
                    // Smoothstep blend
                    let t = edge_dist / blend_width;
                    1.0 - t * t * (3.0 - 2.0 * t)
                }
            } else {
                0.0
            };
            
            // Blend between procedural and flattened height
            let height = procedural_h + (target_height - procedural_h) * blend_factor;
            
            // Position in local coords (relative to center for easier transform)
            positions.push([local_x, height - center.y + 0.05, local_z]); // +0.05 to avoid z-fighting
            normals.push([0.0, 1.0, 0.0]); // Will calculate proper normals later
            uvs.push([xi as f32 / steps_x as f32, zi as f32 / steps_z as f32]);
        }
    }
    
    // Calculate proper normals
    for zi in 0..=steps_z {
        for xi in 0..=steps_x {
            let idx = zi * (steps_x + 1) + xi;
            
            let h_left = if xi > 0 { positions[idx - 1][1] } else { positions[idx][1] };
            let h_right = if xi < steps_x { positions[idx + 1][1] } else { positions[idx][1] };
            let h_down = if zi > 0 { positions[idx - (steps_x + 1)][1] } else { positions[idx][1] };
            let h_up = if zi < steps_z { positions[idx + (steps_x + 1)][1] } else { positions[idx][1] };
            
            let normal = Vec3::new(
                h_left - h_right,
                2.0 * GRID_SPACING,
                h_down - h_up,
            ).normalize();
            
            normals[idx] = [normal.x, normal.y, normal.z];
        }
    }
    
    // Generate indices
    for zi in 0..steps_z {
        for xi in 0..steps_x {
            let top_left = (zi * (steps_x + 1) + xi) as u32;
            let top_right = top_left + 1;
            let bottom_left = top_left + (steps_x + 1) as u32;
            let bottom_right = bottom_left + 1;
            
            indices.push(top_left);
            indices.push(bottom_left);
            indices.push(top_right);
            
            indices.push(top_right);
            indices.push(bottom_left);
            indices.push(bottom_right);
        }
    }
    
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Toggle build mode with B key
fn toggle_build_mode(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut build_state: ResMut<BuildModeState>,
    mut input_state: ResMut<InputState>,
    mut commands: Commands,
) {
    // Don't toggle build mode if inventory is open
    if input_state.inventory_open {
        return;
    }
    
    if keyboard.just_pressed(KeyCode::KeyB) {
        build_state.active = !build_state.active;
        input_state.build_mode_active = build_state.active;
        
        if build_state.active {
            info!("Build mode enabled - move around and look to position building, click to place");
            // Select first building by default
            if build_state.selected_building.is_none() {
                build_state.selected_building = Some(BuildingType::TrainStation);
            }
        } else {
            info!("Build mode disabled");
            cleanup_build_mode_visuals(&mut build_state, &mut commands);
        }
    }
    
    // Escape to exit build mode
    if build_state.active && keyboard.just_pressed(KeyCode::Escape) {
        build_state.active = false;
        input_state.build_mode_active = false;
        cleanup_build_mode_visuals(&mut build_state, &mut commands);
    }
}

/// Handle building selection via number keys or UI clicks
fn handle_building_selection(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut build_state: ResMut<BuildModeState>,
    button_query: Query<(&Interaction, &BuildingSelectButton), Changed<Interaction>>,
) {
    if !build_state.active {
        return;
    }
    
    // Number keys to select buildings
    let building_types = BuildingType::all();
    for (i, building_type) in building_types.iter().enumerate() {
        let key = match i {
            0 => KeyCode::Digit1,
            1 => KeyCode::Digit2,
            2 => KeyCode::Digit3,
            3 => KeyCode::Digit4,
            4 => KeyCode::Digit5,
            5 => KeyCode::Digit6,
            6 => KeyCode::Digit7,
            7 => KeyCode::Digit8,
            8 => KeyCode::Digit9,
            _ => continue,
        };
        
        if keyboard.just_pressed(key) {
            build_state.selected_building = Some(*building_type);
            info!("Selected building: {}", building_type.display_name());
        }
    }
    
    // UI button clicks
    for (interaction, button) in button_query.iter() {
        if *interaction == Interaction::Pressed {
            build_state.selected_building = Some(button.building_type);
            info!("Selected building: {}", button.building_type.display_name());
        }
    }
}

/// Handle rotation input (R key)
fn handle_rotation_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut build_state: ResMut<BuildModeState>,
) {
    if !build_state.active {
        return;
    }
    
    if keyboard.just_pressed(KeyCode::KeyR) {
        build_state.preview_rotation += std::f32::consts::FRAC_PI_2; // 90 degrees
        if build_state.preview_rotation >= std::f32::consts::TAU {
            build_state.preview_rotation -= std::f32::consts::TAU;
        }
    }
}

/// Update the placement preview based on camera look direction
fn update_placement_preview(
    mut commands: Commands,
    mut build_state: ResMut<BuildModeState>,
    assets: Res<BuildModeAssets>,
    terrain: Res<WorldTerrain>,
    mut meshes: ResMut<Assets<Mesh>>,
    camera_query: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    inventory_query: Query<&Inventory, With<LocalPlayer>>,
    mut ghost_query: Query<
        (&mut Transform, &mut Visibility, &mut MeshMaterial3d<StandardMaterial>),
        (With<GhostPreview>, Without<TerrainPreview>),
    >,
    mut terrain_preview_query: Query<
        (&mut Transform, &mut Visibility),
        (With<TerrainPreview>, Without<GhostPreview>),
    >,
) {
    if !build_state.active {
        // Hide preview meshes when not in build mode
        for (_, mut vis, _) in ghost_query.iter_mut() {
            *vis = Visibility::Hidden;
        }
        for (_, mut vis) in terrain_preview_query.iter_mut() {
            *vis = Visibility::Hidden;
        }
        return;
    }
    
    let Some(building_type) = build_state.selected_building else {
        return;
    };
    
    let def = building_type.definition();
    
    // Get camera for raycasting
    let Ok((_camera, camera_transform)) = camera_query.single() else {
        return;
    };
    
    // Raycast from camera center to find terrain intersection
    let ray_origin = camera_transform.translation();
    let ray_direction = camera_transform.forward().as_vec3();
    
    // Simple terrain raycast - step along ray until we hit terrain
    let max_distance = 50.0;
    let step_size = 0.5;
    let mut hit_pos = None;
    
    for i in 0..(max_distance / step_size) as i32 {
        let dist = i as f32 * step_size + 5.0; // Start at 5m to avoid clipping
        let test_pos = ray_origin + ray_direction * dist;
        let terrain_height = terrain.get_height(test_pos.x, test_pos.z);
        
        if test_pos.y <= terrain_height {
            // Found intersection - refine position
            let refined_pos = Vec3::new(test_pos.x, terrain_height, test_pos.z);
            hit_pos = Some(refined_pos);
            break;
        }
    }
    
    let Some(position) = hit_pos else {
        build_state.preview_position = None;
        build_state.can_place = false;
        build_state.invalid_reason = Some("Too far away".to_string());
        return;
    };
    
    build_state.preview_position = Some(position);
    
    // Check if player has required resources
    let can_afford = if let Ok(inventory) = inventory_query.single() {
        def.cost.iter().all(|(item_type, required)| {
            inventory.count_item(*item_type) >= *required
        })
    } else {
        false
    };
    
    if !can_afford {
        build_state.can_place = false;
        build_state.invalid_reason = Some("Not enough resources".to_string());
    } else {
        build_state.can_place = true;
        build_state.invalid_reason = None;
    }
    
    // Spawn or update preview
    let rotation = Quat::from_rotation_y(build_state.preview_rotation);
    let material = if build_state.can_place {
        assets.valid_material.clone()
    } else {
        assets.invalid_material.clone()
    };

    // Check if building type changed - need to respawn model preview
    let building_changed = build_state.previewed_building_type != Some(building_type);
    if building_changed {
        // Despawn old model preview
        if let Some(old_entity) = build_state.model_preview_entity.take() {
            commands.entity(old_entity).despawn();
        }
        build_state.previewed_building_type = Some(building_type);
    }

    // Spawn/update GLTF model preview for buildings with models
    if let Some(scene) = assets.building_scenes.get(&building_type) {
        if build_state.model_preview_entity.is_none() || building_changed {
            let entity = commands.spawn((
                ModelPreview,
                SceneRoot(scene.clone()),
                Transform::from_translation(position).with_rotation(rotation),
            )).id();
            build_state.model_preview_entity = Some(entity);
        }
    }

    // Update model preview position if it exists
    if let Some(entity) = build_state.model_preview_entity {
        if let Ok(mut entity_commands) = commands.get_entity(entity) {
            entity_commands.insert(Transform::from_translation(position).with_rotation(rotation));
        }
    }

    // Spawn/update ghost (footprint box) - only show if no model, or as wireframe indicator
    let has_model = assets.building_scenes.contains_key(&building_type);
    if let Some(mesh) = assets.building_meshes.get(&building_type) {
        if let Ok((mut transform, mut vis, mut mat)) = ghost_query.single_mut() {
            // Update existing ghost
            transform.translation = position;
            transform.rotation = rotation;
            // Hide ghost if we have a model preview (the model is enough)
            *vis = if has_model { Visibility::Hidden } else { Visibility::Visible };
            *mat = MeshMaterial3d(material);
        } else {
            // Spawn new ghost
            if let Some(old_entity) = build_state.ghost_entity {
                commands.entity(old_entity).despawn();
            }

            let entity = commands.spawn((
                GhostPreview,
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(position).with_rotation(rotation),
                if has_model { Visibility::Hidden } else { Visibility::Visible },
            )).id();

            build_state.ghost_entity = Some(entity);
        }
    }
    
    // Update terrain preview with accurate post-flatten mesh
    let half_extents = Vec2::new(def.footprint.x / 2.0, def.footprint.y / 2.0);
    
    let preview_mesh = generate_terrain_preview_mesh(
        &terrain,
        position,
        half_extents,
        build_state.preview_rotation,
        def.flatten_radius,
    );
    
    // Update or create the preview mesh asset in-place (avoid leaking mesh assets each frame)
    let mesh_handle = if let Some(handle) = build_state.terrain_preview_mesh.clone() {
        if let Some(mesh) = meshes.get_mut(&handle) {
            *mesh = preview_mesh;
        }
        handle
    } else {
        let handle = meshes.add(preview_mesh);
        build_state.terrain_preview_mesh = Some(handle.clone());
        handle
    };
    
    // Update or spawn the preview entity
    if let Some(entity) = build_state.terrain_preview_entity {
        if let Ok((mut transform, mut vis)) = terrain_preview_query.get_mut(entity) {
            transform.translation = position;
            transform.rotation = rotation;
            *vis = Visibility::Visible;
        }
    } else {
        let entity = commands.spawn((
            TerrainPreview,
            Mesh3d(mesh_handle),
            MeshMaterial3d(assets.terrain_preview_material.clone()),
            Transform::from_translation(position).with_rotation(rotation),
        )).id();
        
        build_state.terrain_preview_entity = Some(entity);
    }
}

/// Handle placing a building
fn handle_place_building(
    mouse_button: Res<ButtonInput<MouseButton>>,
    build_state: Res<BuildModeState>,
    mut client_query: Query<&mut MessageSender<PlaceBuildingRequest>, (With<crate::GameClient>, With<Connected>)>,
) {
    if !build_state.active || !build_state.can_place {
        return;
    }
    
    if !mouse_button.just_pressed(MouseButton::Left) {
        return;
    }
    
    let Some(building_type) = build_state.selected_building else {
        return;
    };
    
    let Some(position) = build_state.preview_position else {
        return;
    };
    
    // Send place request to server
    let Ok(mut sender) = client_query.single_mut() else {
        warn!("No client connection to send PlaceBuildingRequest");
        return;
    };
    
    let request = PlaceBuildingRequest {
        building_type,
        position,
        rotation: build_state.preview_rotation,
    };
    
    info!("Requesting to place {:?} at {:?}", building_type, position);
    let _ = sender.send::<shared::ReliableChannel>(request);
}

/// Update the build mode UI
fn update_build_mode_ui(
    mut commands: Commands,
    build_state: Res<BuildModeState>,
    ui_query: Query<Entity, With<BuildModeUI>>,
    inventory_query: Query<&Inventory, With<LocalPlayer>>,
) {
    // Despawn UI if build mode is inactive
    if !build_state.active {
        for entity in ui_query.iter() {
            commands.entity(entity).despawn();
        }
        return;
    }
    
    // Only spawn UI once
    if ui_query.iter().count() > 0 {
        return;
    }
    
    let inventory = inventory_query.single().ok();
    
    // Spawn build mode UI
    commands.spawn((
        BuildModeUI,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(20.0),
            top: Val::Px(100.0),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(15.0)),
            row_gap: Val::Px(10.0),
            ..default()
        },
        BackgroundColor(UI_BG),
        BorderColor::from(UI_BORDER),
        BorderRadius::all(Val::Px(8.0)),
    )).with_children(|parent| {
        // Title
        parent.spawn((
            Text::new("BUILD MODE"),
            TextFont {
                font_size: 18.0,
                ..default()
            },
            TextColor(UI_ACCENT),
        ));
        
        // Instructions
        parent.spawn((
            Text::new("Press 1-2 or click to select\nR to rotate | Click to place\nB or ESC to exit"),
            TextFont {
                font_size: 12.0,
                ..default()
            },
            TextColor(UI_TEXT_DIM),
        ));
        
        // Building list
        for (i, building_type) in BuildingType::all().iter().enumerate() {
            let def = building_type.definition();
            let is_selected = build_state.selected_building == Some(*building_type);
            
            let can_afford = if let Some(inv) = inventory {
                def.cost.iter().all(|(item_type, required)| {
                    inv.count_item(*item_type) >= *required
                })
            } else {
                false
            };
            
            let bg_color = if is_selected {
                SLOT_SELECTED
            } else {
                SLOT_NORMAL
            };
            
            parent.spawn((
                BuildingSelectButton { building_type: *building_type },
                Button,
                Node {
                    padding: UiRect::all(Val::Px(10.0)),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                BackgroundColor(bg_color),
                BorderRadius::all(Val::Px(4.0)),
            )).with_children(|btn| {
                // Building name with hotkey
                btn.spawn((
                    Text::new(format!("[{}] {}", i + 1, def.display_name)),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(if can_afford { UI_TEXT } else { UI_TEXT_DIM }),
                ));
                
                // Cost
                let cost_str: String = def.cost.iter()
                    .map(|(item, qty)| {
                        let have = inventory.map(|inv| inv.count_item(*item)).unwrap_or(0);
                        format!("{}: {}/{}", item.display_name(), have, qty)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                
                btn.spawn((
                    Text::new(cost_str),
                    TextFont {
                        font_size: 11.0,
                        ..default()
                    },
                    TextColor(if can_afford { UI_TEXT_DIM } else { Color::srgb(0.8, 0.4, 0.4) }),
                ));
            });
        }
        
        // Current status
        if let Some(reason) = &build_state.invalid_reason {
            parent.spawn((
                Text::new(format!("⚠ {}", reason)),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgb(0.9, 0.5, 0.4)),
            ));
        }
    });
}

/// Clean up build mode visuals
fn cleanup_build_mode_visuals(
    build_state: &mut BuildModeState,
    commands: &mut Commands,
) {
    if let Some(entity) = build_state.ghost_entity.take() {
        commands.entity(entity).despawn();
    }
    if let Some(entity) = build_state.model_preview_entity.take() {
        commands.entity(entity).despawn();
    }
    if let Some(entity) = build_state.terrain_preview_entity.take() {
        commands.entity(entity).despawn();
    }
    build_state.terrain_preview_mesh = None;
    build_state.previewed_building_type = None;
    if let Some(entity) = build_state.ui_entity.take() {
        commands.entity(entity).despawn();
    }
}

/// Spawn visual meshes for replicated PlacedBuilding entities
fn spawn_building_visuals(
    mut commands: Commands,
    assets: Res<BuildModeAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    buildings: Query<(Entity, &PlacedBuilding, &BuildingPosition), Without<BuildingVisual>>,
    existing_visuals: Query<&BuildingVisual>,
) {
    for (entity, building, position) in buildings.iter() {
        // Check if we already have a visual for this building
        let already_has_visual = existing_visuals.iter().any(|v| v.server_entity == entity);
        if already_has_visual {
            continue;
        }

        let def = building.building_type.definition();
        let rotation = Quat::from_rotation_y(building.rotation);

        info!(
            "Spawning visual for {:?} at {:?}",
            building.building_type, position.0
        );

        // Prefer GLTF scene if available, otherwise use fallback mesh
        if let Some(scene) = assets.building_scenes.get(&building.building_type) {
            commands.spawn((
                BuildingVisual { server_entity: entity },
                SceneRoot(scene.clone()),
                Transform::from_translation(position.0).with_rotation(rotation),
            ));
        } else {
            // Fallback to generated mesh
            let Some(mesh) = assets.building_meshes.get(&building.building_type) else {
                warn!("No mesh for building type {:?}", building.building_type);
                continue;
            };

            let material = materials.add(StandardMaterial {
                base_color: def.color,
                perceptual_roughness: 0.8,
                metallic: 0.0,
                ..default()
            });

            commands.spawn((
                BuildingVisual { server_entity: entity },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material),
                Transform::from_translation(position.0).with_rotation(rotation),
            ));
        }
    }
}

/// Clean up building visuals when the server entity is despawned
fn cleanup_building_visuals(
    mut commands: Commands,
    buildings: Query<Entity, With<PlacedBuilding>>,
    visuals: Query<(Entity, &BuildingVisual)>,
) {
    for (visual_entity, visual) in visuals.iter() {
        // Check if the server entity still exists
        if buildings.get(visual.server_entity).is_err() {
            commands.entity(visual_entity).despawn();
        }
    }
}

/// Clean up when leaving gameplay
fn cleanup_build_mode(
    mut commands: Commands,
    mut build_state: ResMut<BuildModeState>,
    ui_query: Query<Entity, With<BuildModeUI>>,
    ghost_query: Query<Entity, With<GhostPreview>>,
    model_preview_query: Query<Entity, With<ModelPreview>>,
    terrain_preview_query: Query<Entity, With<TerrainPreview>>,
    building_visuals: Query<Entity, With<BuildingVisual>>,
) {
    build_state.active = false;
    build_state.selected_building = None;
    build_state.preview_position = None;
    build_state.ghost_entity = None;
    build_state.model_preview_entity = None;
    build_state.previewed_building_type = None;
    build_state.terrain_preview_entity = None;
    build_state.terrain_preview_mesh = None;
    build_state.ui_entity = None;

    for entity in ui_query.iter() {
        commands.entity(entity).despawn();
    }
    for entity in ghost_query.iter() {
        commands.entity(entity).despawn();
    }
    for entity in model_preview_query.iter() {
        commands.entity(entity).despawn();
    }
    for entity in terrain_preview_query.iter() {
        commands.entity(entity).despawn();
    }
    for entity in building_visuals.iter() {
        commands.entity(entity).despawn();
    }
}

/// Draw debug gizmos for building colliders (F4 to toggle)
fn debug_draw_building_colliders(
    mut gizmos: Gizmos,
    debug_mode: Res<WeaponDebugMode>,
    collider_visuals: Res<BuildingColliderVisuals>,
    buildings: Query<(&PlacedBuilding, &BuildingPosition)>,
) {
    if !debug_mode.0 {
        return;
    }

    let color = Color::srgb(0.2, 0.8, 1.0); // Cyan for buildings

    for (building, position) in buildings.iter() {
        let pos = position.0;
        let rotation_quat = Quat::from_rotation_y(building.rotation);

        // Check if we have baked hull edges for this building type
        if let Some(edges) = collider_visuals.hull_edges.get(&building.building_type) {
            // Draw the actual convex hull edges
            for (v0, v1) in edges {
                // Transform local-space edges to world space
                let world_v0 = pos + rotation_quat * *v0;
                let world_v1 = pos + rotation_quat * *v1;
                gizmos.line(world_v0, world_v1, color);
            }
        } else {
            // Fallback to box wireframe for buildings without baked colliders
            let def = building.building_type.definition();
            let rotation = building.rotation;

            let hx = def.footprint.x * 0.5;
            let hz = def.footprint.y * 0.5;

            let cos_r = rotation.cos();
            let sin_r = rotation.sin();

            let rotate_xz = |x: f32, z: f32| -> (f32, f32) {
                (x * cos_r - z * sin_r, x * sin_r + z * cos_r)
            };

            let local_corners = [
                (-hx, 0.0, -hz),
                (hx, 0.0, -hz),
                (hx, 0.0, hz),
                (-hx, 0.0, hz),
            ];

            let mut bottom = [Vec3::ZERO; 4];
            let mut top = [Vec3::ZERO; 4];

            for (i, (lx, _, lz)) in local_corners.iter().enumerate() {
                let (rx, rz) = rotate_xz(*lx, *lz);
                bottom[i] = pos + Vec3::new(rx, 0.0, rz);
                top[i] = pos + Vec3::new(rx, def.height, rz);
            }

            for i in 0..4 {
                gizmos.line(bottom[i], bottom[(i + 1) % 4], color);
            }

            for i in 0..4 {
                gizmos.line(top[i], top[(i + 1) % 4], color);
            }

            // Draw verticals
            for i in 0..4 {
                gizmos.line(bottom[i], top[i], color);
            }
        }
    }
}
