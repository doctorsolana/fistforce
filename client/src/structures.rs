//! Desert settlement structures - client-side rendering
//!
//! Procedurally generates meshes for Dune-inspired buildings.

use bevy::prelude::*;
use bevy::camera::visibility::VisibilityRange;
use bevy::mesh::{Indices, PrimitiveTopology};
use std::collections::{HashMap, HashSet};
use std::f32::consts::PI;

use shared::{
    ChunkCoord, DesertStructureKind, WorldTerrain,
    generate_chunk_structures,
};

use crate::terrain::LoadedChunks;
use crate::systems::ClientWorldRoot;
use crate::states::GameState;
use shared::WeaponDebugMode;

/// Marker for structure entities
#[derive(Component)]
pub struct DesertStructure {
    pub chunk: ChunkCoord,
    #[allow(dead_code)]
    pub kind: DesertStructureKind,
}

/// Tracks which chunks have had structures spawned
#[derive(Resource, Default)]
pub struct LoadedStructureChunks {
    pub chunks: HashSet<ChunkCoord>,
}

/// Pre-generated structure meshes and materials
#[derive(Resource)]
pub struct StructureAssets {
    pub meshes: HashMap<DesertStructureKind, Handle<Mesh>>,
    pub material: Handle<StandardMaterial>,
    pub accent_material: Handle<StandardMaterial>,
    pub lamp_glow_material: Handle<StandardMaterial>,
}

/// Plugin for desert structures
pub struct StructuresPlugin;

impl Plugin for StructuresPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadedStructureChunks>();
        app.add_systems(Startup, setup_structure_assets);
        app.add_systems(
            Update,
            (spawn_chunk_structures, cleanup_chunk_structures, debug_draw_structure_colliders)
                .run_if(in_state(GameState::Playing)),
        );
    }
}

/// Sandy color palette (Dune-inspired)
const SAND_BASE: Color = Color::srgb(0.83, 0.69, 0.51);      // #D4B082
const SAND_ACCENT: Color = Color::srgb(0.77, 0.59, 0.42);    // #C4966B

/// Ensure triangle winding matches the intended vertex normals.
///
/// Bevy (and GPUs) use **triangle winding** for backface culling, not the normal attribute.
/// In procedural meshes it's easy to get a few quads inverted; this post-pass fixes that
/// automatically by flipping triangles whose geometric normal points opposite the average
/// of their vertex normals.
fn fix_winding_against_vertex_normals(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    indices: &mut [u32],
) {
    if positions.len() != normals.len() {
        // If we ever diverge, don't guess.
        return;
    }

    for tri in indices.chunks_exact_mut(3) {
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;
        if i0 >= positions.len() || i1 >= positions.len() || i2 >= positions.len() {
            continue;
        }

        let p0 = Vec3::from(positions[i0]);
        let p1 = Vec3::from(positions[i1]);
        let p2 = Vec3::from(positions[i2]);

        let ng = (p1 - p0).cross(p2 - p0);
        let ng_len2 = ng.length_squared();
        if ng_len2 < 1e-10 {
            continue;
        }
        let ng = ng / ng_len2.sqrt();

        let n0 = Vec3::from(normals[i0]);
        let n1 = Vec3::from(normals[i1]);
        let n2 = Vec3::from(normals[i2]);
        let na = n0 + n1 + n2;
        let na_len2 = na.length_squared();
        if na_len2 < 1e-10 {
            continue;
        }
        let na = na / na_len2.sqrt();

        // If the triangle is wound "inside out" relative to intended normals, flip it.
        if ng.dot(na) < 0.0 {
            tri.swap(1, 2);
        }
    }
}

/// Generate all structure meshes at startup
fn setup_structure_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut mesh_handles = HashMap::new();

    // Generate each structure type's mesh
    mesh_handles.insert(
        DesertStructureKind::SmallDome,
        meshes.add(generate_dome_mesh(3.5, 4.0, 24, 12)),
    );
    mesh_handles.insert(
        DesertStructureKind::LargeDome,
        meshes.add(generate_dome_mesh(7.0, 8.0, 32, 16)),
    );
    mesh_handles.insert(
        DesertStructureKind::CurvedWall,
        meshes.add(generate_curved_wall_mesh(8.0, 3.5, 0.8, 16)),
    );
    mesh_handles.insert(
        DesertStructureKind::WatchTower,
        meshes.add(generate_tower_mesh(2.5, 12.0, 20)),
    );
    mesh_handles.insert(
        DesertStructureKind::Archway,
        meshes.add(generate_archway_mesh(6.0, 5.0, 2.0, 1.0)),
    );
    mesh_handles.insert(
        DesertStructureKind::StorageSilo,
        meshes.add(generate_silo_mesh(2.0, 5.0, 16)),
    );
    mesh_handles.insert(
        DesertStructureKind::DesertLamp,
        meshes.add(generate_lamp_mesh(0.15, 2.5, 12)),
    );

    // Create sandy/adobe materials
    let material = materials.add(StandardMaterial {
        base_color: SAND_BASE,
        perceptual_roughness: 0.85,
        metallic: 0.0,
        ..default()
    });

    let accent_material = materials.add(StandardMaterial {
        base_color: SAND_ACCENT,
        perceptual_roughness: 0.9,
        metallic: 0.0,
        ..default()
    });

    // Warm glowing material for lamp tops (emissive orange)
    let lamp_glow_material = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.7, 0.3),
        emissive: bevy::color::LinearRgba::new(3.0, 1.5, 0.5, 1.0),
        perceptual_roughness: 0.3,
        metallic: 0.0,
        ..default()
    });

    commands.insert_resource(StructureAssets {
        meshes: mesh_handles,
        material,
        accent_material,
        lamp_glow_material,
    });

    info!("Generated desert structure meshes and materials");
}

/// Generate a dome mesh (hemisphere with organic look)
fn generate_dome_mesh(radius: f32, height: f32, segments: usize, rings: usize) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    // Generate vertices for hemisphere
    for ring in 0..=rings {
        let v = ring as f32 / rings as f32;
        let phi = v * PI * 0.5; // 0 to 90 degrees (hemisphere)

        for seg in 0..=segments {
            let u = seg as f32 / segments as f32;
            let theta = u * PI * 2.0;

            // Spherical coordinates with height scaling
            let x = radius * phi.sin() * theta.cos();
            let z = radius * phi.sin() * theta.sin();
            let y = height * phi.cos() * (1.0 - phi.cos() * 0.3); // Slightly flatten top

            // Add slight organic bulge variation
            let bulge = 1.0 + 0.03 * ((theta * 3.0).sin() * (phi * 2.0).cos());
            let x = x * bulge;
            let z = z * bulge;

            positions.push([x, y, z]);

            // Normal points outward
            let normal = Vec3::new(x, y / height * radius, z).normalize();
            normals.push([normal.x, normal.y, normal.z]);

            uvs.push([u, v]);
        }
    }

    // Generate indices (counter-clockwise winding for outward-facing)
    for ring in 0..rings {
        for seg in 0..segments {
            let current = ring * (segments + 1) + seg;
            let next = current + segments + 1;

            // Triangle 1: current -> current+1 -> next (CCW from outside)
            indices.push(current as u32);
            indices.push((current + 1) as u32);
            indices.push(next as u32);

            // Triangle 2: current+1 -> next+1 -> next (CCW from outside)
            indices.push((current + 1) as u32);
            indices.push((next + 1) as u32);
            indices.push(next as u32);
        }
    }

    // Add bottom cap (floor) - winding for downward-facing
    let center_idx = positions.len() as u32;
    positions.push([0.0, 0.0, 0.0]);
    normals.push([0.0, -1.0, 0.0]);
    uvs.push([0.5, 0.5]);

    for seg in 0..segments {
        let idx1 = seg as u32;
        let idx2 = ((seg + 1) % (segments + 1)) as u32;
        indices.push(center_idx);
        indices.push(idx1);
        indices.push(idx2);
    }

    fix_winding_against_vertex_normals(&positions, &normals, &mut indices);
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Generate a curved wall segment
fn generate_curved_wall_mesh(length: f32, height: f32, thickness: f32, segments: usize) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let half_length = length / 2.0;
    let half_thick = thickness / 2.0;
    let curve_amount = 0.15; // Slight curve

    // Generate curved wall vertices (front and back faces)
    for face in 0..2 {
        let z_offset = if face == 0 { half_thick } else { -half_thick };
        let normal_z = if face == 0 { 1.0 } else { -1.0 };

        for seg in 0..=segments {
            let t = seg as f32 / segments as f32;
            let x = -half_length + t * length;

            // Curve profile
            let curve = curve_amount * (t * PI).sin();

            for vy in 0..=2 {
                let y_t = vy as f32 / 2.0;
                let y = y_t * height;

                // Taper at top
                let taper = 1.0 - y_t * 0.1;

                positions.push([x, y, (z_offset + curve) * taper]);

                let normal = Vec3::new(0.0, 0.0, normal_z).normalize();
                normals.push([normal.x, normal.y, normal.z]);

                uvs.push([t, y_t]);
            }
        }
    }

    // Generate indices for front and back faces
    // Front face (z+): CCW winding when viewed from +z
    // Back face (z-): CCW winding when viewed from -z (opposite)
    let verts_per_face = (segments + 1) * 3;
    for face in 0..2 {
        let base = face * verts_per_face;
        for seg in 0..segments {
            for vy in 0..2 {
                let current = base + seg * 3 + vy;
                let next_seg = base + (seg + 1) * 3 + vy;

                if face == 0 {
                    // Front face (+Z normal) - CCW from outside
                    indices.push(current as u32);
                    indices.push(next_seg as u32);
                    indices.push((current + 1) as u32);

                    indices.push((current + 1) as u32);
                    indices.push(next_seg as u32);
                    indices.push((next_seg + 1) as u32);
                } else {
                    // Back face (-Z normal) - CCW from outside (reversed)
                    indices.push(current as u32);
                    indices.push((current + 1) as u32);
                    indices.push(next_seg as u32);

                    indices.push((current + 1) as u32);
                    indices.push((next_seg + 1) as u32);
                    indices.push(next_seg as u32);
                }
            }
        }
    }

    // Add top cap (upward facing)
    let top_base = positions.len() as u32;
    for seg in 0..=segments {
        let t = seg as f32 / segments as f32;
        let x = -half_length + t * length;
        let curve = curve_amount * (t * PI).sin();
        let taper = 0.9;

        positions.push([x, height, (half_thick + curve) * taper]);
        positions.push([x, height, (-half_thick + curve) * taper]);
        normals.push([0.0, 1.0, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([t, 0.0]);
        uvs.push([t, 1.0]);
    }

    for seg in 0..segments {
        let base = top_base + (seg * 2) as u32;
        // Top cap - CCW when viewed from above (+Y)
        // NOTE: Keep winding consistent with +Y normals (otherwise cap looks inverted with backface culling)
        indices.push(base);
        indices.push(base + 2);
        indices.push(base + 1);

        indices.push(base + 1);
        indices.push(base + 2);
        indices.push(base + 3);
    }

    // Add left edge cap (at x = -half_length)
    let left_edge_base = positions.len() as u32;
    let left_curve = curve_amount * 0.0_f32.sin(); // curve at t=0
    for vy in 0..=2 {
        let y_t = vy as f32 / 2.0;
        let y = y_t * height;
        let taper = 1.0 - y_t * 0.1;
        // Front vertex
        positions.push([-half_length, y, (half_thick + left_curve) * taper]);
        normals.push([-1.0, 0.0, 0.0]);
        uvs.push([0.0, y_t]);
        // Back vertex
        positions.push([-half_length, y, (-half_thick + left_curve) * taper]);
        normals.push([-1.0, 0.0, 0.0]);
        uvs.push([1.0, y_t]);
    }
    // Left edge triangles (facing -X)
    for vy in 0..2 {
        let base = left_edge_base + (vy * 2) as u32;
        // CCW when viewed from -X direction (reversed)
        indices.push(base);
        indices.push(base + 2);
        indices.push(base + 1);

        indices.push(base + 1);
        indices.push(base + 2);
        indices.push(base + 3);
    }

    // Add right edge cap (at x = +half_length)
    let right_edge_base = positions.len() as u32;
    let right_curve = curve_amount * PI.sin(); // curve at t=1
    for vy in 0..=2 {
        let y_t = vy as f32 / 2.0;
        let y = y_t * height;
        let taper = 1.0 - y_t * 0.1;
        // Front vertex
        positions.push([half_length, y, (half_thick + right_curve) * taper]);
        normals.push([1.0, 0.0, 0.0]);
        uvs.push([0.0, y_t]);
        // Back vertex
        positions.push([half_length, y, (-half_thick + right_curve) * taper]);
        normals.push([1.0, 0.0, 0.0]);
        uvs.push([1.0, y_t]);
    }
    // Right edge triangles (facing +X)
    for vy in 0..2 {
        let base = right_edge_base + (vy * 2) as u32;
        // CCW when viewed from +X direction
        indices.push(base);
        indices.push(base + 1);
        indices.push(base + 2);

        indices.push(base + 1);
        indices.push(base + 3);
        indices.push(base + 2);
    }

    fix_winding_against_vertex_normals(&positions, &normals, &mut indices);
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Generate a watchtower mesh (tapered cylinder with platform)
fn generate_tower_mesh(radius: f32, height: f32, segments: usize) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let rings = 8;
    let top_radius = radius * 0.7;

    // Main tower body
    for ring in 0..=rings {
        let t = ring as f32 / rings as f32;
        let y = t * height;

        // Taper towards top
        let r = radius * (1.0 - t * 0.3);

        // Add slight bulge in middle
        let bulge = 1.0 + 0.05 * (t * PI).sin();
        let r = r * bulge;

        for seg in 0..=segments {
            let theta = (seg as f32 / segments as f32) * PI * 2.0;
            let x = r * theta.cos();
            let z = r * theta.sin();

            positions.push([x, y, z]);

            let normal = Vec3::new(theta.cos(), 0.2, theta.sin()).normalize();
            normals.push([normal.x, normal.y, normal.z]);

            uvs.push([seg as f32 / segments as f32, t]);
        }
    }

    // Generate indices for tower body (CCW winding for outward-facing)
    for ring in 0..rings {
        for seg in 0..segments {
            let current = ring * (segments + 1) + seg;
            let next = current + segments + 1;

            // CCW from outside (reversed order)
            indices.push(current as u32);
            indices.push(next as u32);
            indices.push((current + 1) as u32);

            indices.push((current + 1) as u32);
            indices.push(next as u32);
            indices.push((next + 1) as u32);
        }
    }

    // Add dome roof at top of tower
    let dome_base = positions.len() as u32;
    let dome_radius = top_radius * 1.1;
    let dome_height = radius * 0.8;
    let dome_rings = 6;
    let dome_start_y = height;

    for ring in 0..=dome_rings {
        let t = ring as f32 / dome_rings as f32;
        let phi = t * PI * 0.5; // Half sphere (0 to 90 degrees)

        let r = dome_radius * phi.cos();
        let y = dome_start_y + dome_height * phi.sin();

        for seg in 0..=segments {
            let theta = (seg as f32 / segments as f32) * PI * 2.0;
            let x = r * theta.cos();
            let z = r * theta.sin();

            positions.push([x, y, z]);

            // Outward-pointing normal for dome
            let normal = Vec3::new(
                phi.cos() * theta.cos(),
                phi.sin(),
                phi.cos() * theta.sin(),
            ).normalize();
            normals.push([normal.x, normal.y, normal.z]);
            uvs.push([seg as f32 / segments as f32, t]);
        }
    }

    // Dome indices
    for ring in 0..dome_rings {
        for seg in 0..segments {
            let current = dome_base + (ring * (segments + 1) + seg) as u32;
            let next = current + (segments + 1) as u32;

            indices.push(current);
            indices.push(next);
            indices.push(current + 1);

            indices.push(current + 1);
            indices.push(next);
            indices.push(next + 1);
        }
    }

    // Add bottom cap for dome (underside facing down)
    let dome_cap_center = positions.len() as u32;
    positions.push([0.0, dome_start_y, 0.0]);
    normals.push([0.0, -1.0, 0.0]);
    uvs.push([0.5, 0.5]);

    // Bottom cap uses the first ring vertices (ring 0) from the dome
    for seg in 0..segments {
        let idx1 = dome_base + seg as u32;
        let idx2 = dome_base + (seg + 1) as u32;
        // CCW when viewed from below (looking up at underside)
        indices.push(dome_cap_center);
        indices.push(idx2);
        indices.push(idx1);
    }

    fix_winding_against_vertex_normals(&positions, &normals, &mut indices);
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Generate an archway mesh
fn generate_archway_mesh(width: f32, height: f32, depth: f32, thickness: f32) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let half_width = width / 2.0;
    let half_depth = depth / 2.0;
    let arch_segments = 12;
    let pillar_height = height * 0.6;
    let arch_radius = half_width;

    // Left pillar
    add_box(
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut indices,
        Vec3::new(-half_width + thickness / 2.0, pillar_height / 2.0, 0.0),
        Vec3::new(thickness / 2.0, pillar_height / 2.0, half_depth),
    );

    // Right pillar
    add_box(
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut indices,
        Vec3::new(half_width - thickness / 2.0, pillar_height / 2.0, 0.0),
        Vec3::new(thickness / 2.0, pillar_height / 2.0, half_depth),
    );

    // Arch (half cylinder)
    let arch_base = positions.len() as u32;
    for seg in 0..=arch_segments {
        let t = seg as f32 / arch_segments as f32;
        let theta = t * PI; // Half circle

        let x = -arch_radius * theta.cos();
        let y = pillar_height + arch_radius * theta.sin();

        // Outer arch
        for dz in [-half_depth, half_depth] {
            positions.push([x, y, dz]);
            let normal = Vec3::new(-theta.cos(), theta.sin(), 0.0).normalize();
            normals.push([normal.x, normal.y, normal.z]);
            uvs.push([t, if dz < 0.0 { 0.0 } else { 1.0 }]);
        }
    }

    // Arch indices (outer surface)
    for seg in 0..arch_segments {
        let base = arch_base + seg * 2;
        // CCW winding from outside (match outward normals); otherwise the arch looks inverted with backface culling
        indices.push(base);
        indices.push(base + 2);
        indices.push(base + 1);

        indices.push(base + 1);
        indices.push(base + 2);
        indices.push(base + 3);
    }

    // Inner arch (smaller radius)
    let inner_radius = arch_radius - thickness;
    let inner_base = positions.len() as u32;
    for seg in 0..=arch_segments {
        let t = seg as f32 / arch_segments as f32;
        let theta = t * PI;

        let x = -inner_radius * theta.cos();
        let y = pillar_height + inner_radius * theta.sin();

        for dz in [-half_depth, half_depth] {
            positions.push([x, y, dz]);
            let normal = Vec3::new(theta.cos(), -theta.sin(), 0.0).normalize();
            normals.push([normal.x, normal.y, normal.z]);
            uvs.push([t, if dz < 0.0 { 0.0 } else { 1.0 }]);
        }
    }

    // Inner arch indices (facing inward)
    for seg in 0..arch_segments {
        let base = inner_base + seg * 2;
        indices.push(base);
        indices.push(base + 2);
        indices.push(base + 1);

        indices.push(base + 1);
        indices.push(base + 2);
        indices.push(base + 3);
    }

    // Front cap (z = +half_depth) - connects outer to inner arch
    // Need new vertices with +Z normals
    let front_cap_base = positions.len() as u32;
    for seg in 0..=arch_segments {
        let t = seg as f32 / arch_segments as f32;
        let theta = t * PI;

        let outer_x = -arch_radius * theta.cos();
        let outer_y = pillar_height + arch_radius * theta.sin();
        let inner_x = -inner_radius * theta.cos();
        let inner_y = pillar_height + inner_radius * theta.sin();

        // Outer vertex (front)
        positions.push([outer_x, outer_y, half_depth]);
        normals.push([0.0, 0.0, 1.0]);
        uvs.push([t, 0.0]);

        // Inner vertex (front)
        positions.push([inner_x, inner_y, half_depth]);
        normals.push([0.0, 0.0, 1.0]);
        uvs.push([t, 1.0]);
    }

    // Front cap indices (CCW when viewed from +Z)
    for seg in 0..arch_segments {
        let base = front_cap_base + seg * 2;
        indices.push(base);
        indices.push(base + 1);
        indices.push(base + 2);

        indices.push(base + 1);
        indices.push(base + 3);
        indices.push(base + 2);
    }

    // Back cap (z = -half_depth) - connects outer to inner arch
    let back_cap_base = positions.len() as u32;
    for seg in 0..=arch_segments {
        let t = seg as f32 / arch_segments as f32;
        let theta = t * PI;

        let outer_x = -arch_radius * theta.cos();
        let outer_y = pillar_height + arch_radius * theta.sin();
        let inner_x = -inner_radius * theta.cos();
        let inner_y = pillar_height + inner_radius * theta.sin();

        // Outer vertex (back)
        positions.push([outer_x, outer_y, -half_depth]);
        normals.push([0.0, 0.0, -1.0]);
        uvs.push([t, 0.0]);

        // Inner vertex (back)
        positions.push([inner_x, inner_y, -half_depth]);
        normals.push([0.0, 0.0, -1.0]);
        uvs.push([t, 1.0]);
    }

    // Back cap indices (CCW when viewed from -Z, so reversed)
    for seg in 0..arch_segments {
        let base = back_cap_base + seg * 2;
        indices.push(base);
        indices.push(base + 2);
        indices.push(base + 1);

        indices.push(base + 1);
        indices.push(base + 2);
        indices.push(base + 3);
    }

    fix_winding_against_vertex_normals(&positions, &normals, &mut indices);
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Generate a storage silo mesh (cylinder with domed top)
fn generate_silo_mesh(radius: f32, height: f32, segments: usize) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let cylinder_height = height * 0.7;
    let dome_height = height * 0.3;

    // Cylinder body
    for vy in 0..=4 {
        let t = vy as f32 / 4.0;
        let y = t * cylinder_height;

        for seg in 0..=segments {
            let theta = (seg as f32 / segments as f32) * PI * 2.0;
            let x = radius * theta.cos();
            let z = radius * theta.sin();

            positions.push([x, y, z]);
            normals.push([theta.cos(), 0.0, theta.sin()]);
            uvs.push([seg as f32 / segments as f32, t]);
        }
    }

    // Cylinder indices (CCW for outward-facing)
    for ring in 0..4 {
        for seg in 0..segments {
            let current = ring * (segments + 1) + seg;
            let next = current + segments + 1;

            // CCW from outside (reversed order)
            indices.push(current as u32);
            indices.push(next as u32);
            indices.push((current + 1) as u32);

            indices.push((current + 1) as u32);
            indices.push(next as u32);
            indices.push((next + 1) as u32);
        }
    }

    // Dome top
    let dome_base = positions.len() as u32;
    let dome_rings = 6;
    for ring in 0..=dome_rings {
        let t = ring as f32 / dome_rings as f32;
        let phi = t * PI * 0.5;

        let r = radius * phi.cos();
        let y = cylinder_height + dome_height * phi.sin();

        for seg in 0..=segments {
            let theta = (seg as f32 / segments as f32) * PI * 2.0;
            let x = r * theta.cos();
            let z = r * theta.sin();

            positions.push([x, y, z]);

            let normal = Vec3::new(
                phi.cos() * theta.cos(),
                phi.sin(),
                phi.cos() * theta.sin(),
            ).normalize();
            normals.push([normal.x, normal.y, normal.z]);
            uvs.push([seg as f32 / segments as f32, t]);
        }
    }

    // Dome indices (CCW for outward-facing)
    for ring in 0..dome_rings {
        for seg in 0..segments {
            let current = dome_base + (ring * (segments + 1) + seg) as u32;
            let next = current + (segments + 1) as u32;

            // CCW from outside (reversed order)
            indices.push(current);
            indices.push(next);
            indices.push(current + 1);

            indices.push(current + 1);
            indices.push(next);
            indices.push(next + 1);
        }
    }

    fix_winding_against_vertex_normals(&positions, &normals, &mut indices);
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Generate a desert lamp post mesh (thin pole with decorative top)
fn generate_lamp_mesh(radius: f32, height: f32, segments: usize) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();

    let pole_height = height * 0.8;
    let lamp_height = height * 0.2;
    let pole_radius = radius;
    let lamp_radius = radius * 2.5;

    // Pole body (tapered cylinder)
    for vy in 0..=4 {
        let t = vy as f32 / 4.0;
        let y = t * pole_height;
        let r = pole_radius * (1.0 - t * 0.3); // Slight taper

        for seg in 0..=segments {
            let theta = (seg as f32 / segments as f32) * PI * 2.0;
            let x = r * theta.cos();
            let z = r * theta.sin();

            positions.push([x, y, z]);
            normals.push([theta.cos(), 0.0, theta.sin()]);
            uvs.push([seg as f32 / segments as f32, t]);
        }
    }

    // Pole indices (CCW for outward-facing)
    for ring in 0..4 {
        for seg in 0..segments {
            let current = ring * (segments + 1) + seg;
            let next = current + segments + 1;

            indices.push(current as u32);
            indices.push(next as u32);
            indices.push((current + 1) as u32);

            indices.push((current + 1) as u32);
            indices.push(next as u32);
            indices.push((next + 1) as u32);
        }
    }

    // Decorative lamp top (small dome/bulb shape)
    let lamp_base = positions.len() as u32;
    let lamp_rings = 5;
    let lamp_center_y = pole_height;

    for ring in 0..=lamp_rings {
        let t = ring as f32 / lamp_rings as f32;
        // Create a rounded bulb shape going from bottom to top
        let phi = t * PI;
        let r = lamp_radius * phi.sin();
        let y = lamp_center_y + lamp_height * (1.0 - phi.cos()) * 0.5;

        for seg in 0..=segments {
            let theta = (seg as f32 / segments as f32) * PI * 2.0;
            let x = r * theta.cos();
            let z = r * theta.sin();

            positions.push([x, y, z]);

            // Outward normal: at phi=0 (bottom) points down, at phi=PI (top) points up
            let normal = Vec3::new(
                phi.sin() * theta.cos(),
                -phi.cos(),  // Negated for correct outward direction
                phi.sin() * theta.sin(),
            ).normalize();
            normals.push([normal.x, normal.y, normal.z]);
            uvs.push([seg as f32 / segments as f32, t]);
        }
    }

    // Lamp top indices (CCW for outward-facing)
    for ring in 0..lamp_rings {
        for seg in 0..segments {
            let current = lamp_base + (ring * (segments + 1) + seg) as u32;
            let next = current + (segments + 1) as u32;

            indices.push(current);
            indices.push(next);
            indices.push(current + 1);

            indices.push(current + 1);
            indices.push(next);
            indices.push(next + 1);
        }
    }

    fix_winding_against_vertex_normals(&positions, &normals, &mut indices);
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    mesh
}

/// Helper to add a box to mesh data
fn add_box(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    center: Vec3,
    half_extents: Vec3,
) {
    let base_idx = positions.len() as u32;

    // Define 6 faces with their normals
    let faces = [
        // +X
        ([1.0, 0.0, 0.0], [
            [half_extents.x, -half_extents.y, -half_extents.z],
            [half_extents.x, half_extents.y, -half_extents.z],
            [half_extents.x, half_extents.y, half_extents.z],
            [half_extents.x, -half_extents.y, half_extents.z],
        ]),
        // -X
        ([-1.0, 0.0, 0.0], [
            [-half_extents.x, -half_extents.y, half_extents.z],
            [-half_extents.x, half_extents.y, half_extents.z],
            [-half_extents.x, half_extents.y, -half_extents.z],
            [-half_extents.x, -half_extents.y, -half_extents.z],
        ]),
        // +Y
        ([0.0, 1.0, 0.0], [
            [-half_extents.x, half_extents.y, -half_extents.z],
            [-half_extents.x, half_extents.y, half_extents.z],
            [half_extents.x, half_extents.y, half_extents.z],
            [half_extents.x, half_extents.y, -half_extents.z],
        ]),
        // -Y
        ([0.0, -1.0, 0.0], [
            [-half_extents.x, -half_extents.y, half_extents.z],
            [-half_extents.x, -half_extents.y, -half_extents.z],
            [half_extents.x, -half_extents.y, -half_extents.z],
            [half_extents.x, -half_extents.y, half_extents.z],
        ]),
        // +Z
        ([0.0, 0.0, 1.0], [
            [-half_extents.x, -half_extents.y, half_extents.z],
            [half_extents.x, -half_extents.y, half_extents.z],
            [half_extents.x, half_extents.y, half_extents.z],
            [-half_extents.x, half_extents.y, half_extents.z],
        ]),
        // -Z
        ([0.0, 0.0, -1.0], [
            [half_extents.x, -half_extents.y, -half_extents.z],
            [-half_extents.x, -half_extents.y, -half_extents.z],
            [-half_extents.x, half_extents.y, -half_extents.z],
            [half_extents.x, half_extents.y, -half_extents.z],
        ]),
    ];

    for (i, (normal, verts)) in faces.iter().enumerate() {
        let face_base = base_idx + (i * 4) as u32;

        for (j, vert) in verts.iter().enumerate() {
            positions.push([center.x + vert[0], center.y + vert[1], center.z + vert[2]]);
            normals.push(*normal);
            uvs.push([
                if j == 0 || j == 3 { 0.0 } else { 1.0 },
                if j == 0 || j == 1 { 0.0 } else { 1.0 },
            ]);
        }

        indices.push(face_base);
        indices.push(face_base + 1);
        indices.push(face_base + 2);
        indices.push(face_base);
        indices.push(face_base + 2);
        indices.push(face_base + 3);
    }
}

/// Spawn structures for newly loaded terrain chunks
fn spawn_chunk_structures(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    assets: Option<Res<StructureAssets>>,
    loaded_chunks: Res<LoadedChunks>,
    mut loaded_structure_chunks: ResMut<LoadedStructureChunks>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
) {
    let Some(assets) = assets else { return };
    let Ok(world_root) = world_root_query.single() else { return };

    for coord in loaded_chunks.chunks.iter() {
        if loaded_structure_chunks.chunks.contains(coord) {
            continue;
        }

        let spawns = generate_chunk_structures(&terrain.generator, *coord);
        
        for spawn in spawns {
            let Some(mesh) = assets.meshes.get(&spawn.kind).cloned() else {
                continue;
            };

            // Choose material based on structure type
            let material = match spawn.kind {
                DesertStructureKind::DesertLamp => assets.lamp_glow_material.clone(),
                _ if spawn.scale > 1.0 => assets.material.clone(),
                _ => assets.accent_material.clone(),
            };

            let structure = commands
                .spawn((
                    DesertStructure {
                        chunk: ChunkCoord::from_world_pos(spawn.position),
                        kind: spawn.kind,
                    },
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::from_translation(spawn.position)
                        .with_rotation(spawn.rotation)
                        .with_scale(Vec3::splat(spawn.scale)),
                    // Visibility range for LOD (structures visible from far away)
                    VisibilityRange::abrupt(0.0, 300.0),
                ))
                .id();
            
            // Add warm point light for lamps
            if spawn.kind == DesertStructureKind::DesertLamp {
                let lamp_height = 2.5 * spawn.scale; // Match lamp mesh height
                let light = commands.spawn((
                    PointLight {
                        color: Color::srgb(1.0, 0.8, 0.4), // Warm orange-yellow
                        intensity: 8000.0, // Lumens - cozy glow
                        range: 15.0,
                        radius: 0.3,
                        shadows_enabled: false, // Keep performance reasonable
                        ..default()
                    },
                    Transform::from_xyz(0.0, lamp_height * 0.85, 0.0), // Position at lamp bulb
                )).id();
                commands.entity(structure).add_child(light);
            }
            
            commands.entity(world_root).add_child(structure);
        }

        loaded_structure_chunks.chunks.insert(*coord);
    }
}

/// Clean up structures for unloaded chunks
fn cleanup_chunk_structures(
    mut commands: Commands,
    loaded_chunks: Res<LoadedChunks>,
    mut loaded_structure_chunks: ResMut<LoadedStructureChunks>,
    structures: Query<(Entity, &DesertStructure)>,
) {
    // Find chunks that are no longer loaded
    let to_remove: Vec<ChunkCoord> = loaded_structure_chunks
        .chunks
        .iter()
        .filter(|c| !loaded_chunks.chunks.contains(*c))
        .copied()
        .collect();

    for coord in to_remove {
        // Despawn all structures in this chunk
        for (entity, structure) in structures.iter() {
            if structure.chunk == coord {
                commands.entity(entity).despawn();
            }
        }
        loaded_structure_chunks.chunks.remove(&coord);
    }
}

/// Draw debug gizmos for structure colliders
fn debug_draw_structure_colliders(
    mut gizmos: Gizmos,
    debug_mode: Res<WeaponDebugMode>,
    camera: Query<&Transform, With<Camera3d>>,
    structures: Query<(&DesertStructure, &Transform)>,
) {
    use shared::StructureCollider;
    
    if !debug_mode.0 {
        return;
    }

    let Ok(camera) = camera.single() else { return };
    let cam_pos = camera.translation;
    let max_dist = 150.0;
    let max_dist2 = max_dist * max_dist;

    let color = Color::srgba(1.0, 0.5, 0.0, 0.6); // Orange for structures

    for (structure, transform) in structures.iter() {
        let pos = transform.translation;
        if (pos - cam_pos).length_squared() > max_dist2 {
            continue;
        }

        let scale = transform.scale.x;
        let collider = structure.kind.collider();

        match collider {
            StructureCollider::Dome { radius, height } => {
                // Draw dome as horizontal circles at different heights
                // Use effective height (70% due to flattening in mesh)
                let effective_height = height * 0.7 * scale;
                let r = radius * scale;
                
                for i in 0..6 {
                    let t = i as f32 / 5.0;
                    let y = effective_height * (1.0 - t);
                    let ring_r = r * (1.0 - (1.0 - t).powi(2)).sqrt(); // hemisphere profile
                    
                    let circle_pos = pos + Vec3::Y * y;
                    let iso = Isometry3d::new(circle_pos, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
                    gizmos.circle(iso, ring_r, color).resolution(24);
                }
            }
            StructureCollider::Cylinder { radius, height } => {
                let r = radius * scale;
                let h = height * scale;
                
                // Bottom circle
                let iso_bottom = Isometry3d::new(pos, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
                gizmos.circle(iso_bottom, r, color).resolution(20);
                
                // Top circle
                let iso_top = Isometry3d::new(pos + Vec3::Y * h, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2));
                gizmos.circle(iso_top, r, color).resolution(20);
                
                // Vertical lines
                for i in 0..8 {
                    let angle = (i as f32 / 8.0) * std::f32::consts::TAU;
                    let offset = Vec3::new(angle.cos() * r, 0.0, angle.sin() * r);
                    gizmos.line(pos + offset, pos + offset + Vec3::Y * h, color);
                }
            }
            StructureCollider::Box { half_extents } => {
                let he = half_extents * scale;
                let min = pos - Vec3::new(he.x, 0.0, he.z);
                let max = pos + Vec3::new(he.x, he.y * 2.0, he.z);
                
                // Draw box wireframe
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
                
                // Bottom face
                gizmos.line(corners[0], corners[1], color);
                gizmos.line(corners[1], corners[2], color);
                gizmos.line(corners[2], corners[3], color);
                gizmos.line(corners[3], corners[0], color);
                
                // Top face
                gizmos.line(corners[4], corners[5], color);
                gizmos.line(corners[5], corners[6], color);
                gizmos.line(corners[6], corners[7], color);
                gizmos.line(corners[7], corners[4], color);
                
                // Verticals
                for i in 0..4 {
                    gizmos.line(corners[i], corners[i + 4], color);
                }
            }
            StructureCollider::Arch { width, height, depth, thickness } => {
                let hw = width * 0.5 * scale;
                let hd = depth * 0.5 * scale;
                let th = thickness * scale;
                let h = height * scale;
                
                // Draw left pillar
                let left_min = pos + Vec3::new(-hw, 0.0, -hd);
                let left_max = pos + Vec3::new(-hw + th, h * 0.7, hd);
                draw_box_wireframe(&mut gizmos, left_min, left_max, color);
                
                // Draw right pillar
                let right_min = pos + Vec3::new(hw - th, 0.0, -hd);
                let right_max = pos + Vec3::new(hw, h * 0.7, hd);
                draw_box_wireframe(&mut gizmos, right_min, right_max, color);
                
                // Draw top
                let top_min = pos + Vec3::new(-hw, h * 0.6, -hd);
                let top_max = pos + Vec3::new(hw, h, hd);
                draw_box_wireframe(&mut gizmos, top_min, top_max, color);
            }
        }
    }
}

fn draw_box_wireframe(gizmos: &mut Gizmos, min: Vec3, max: Vec3, color: Color) {
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
    
    // Bottom
    gizmos.line(corners[0], corners[1], color);
    gizmos.line(corners[1], corners[2], color);
    gizmos.line(corners[2], corners[3], color);
    gizmos.line(corners[3], corners[0], color);
    
    // Top
    gizmos.line(corners[4], corners[5], color);
    gizmos.line(corners[5], corners[6], color);
    gizmos.line(corners[6], corners[7], color);
    gizmos.line(corners[7], corners[4], color);
    
    // Verticals
    for i in 0..4 {
        gizmos.line(corners[i], corners[i + 4], color);
    }
}
