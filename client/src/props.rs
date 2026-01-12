//! Environmental props - rocks, trees, grass, etc.
//!
//! Spawns decorative assets based on biome type using deterministic placement.

use bevy::prelude::*;
use bevy::camera::visibility::VisibilityRange;
use bevy::light::NotShadowCaster;
use shared::{ChunkCoord, WorldTerrain};
use shared::PropRenderTuning;
use std::collections::HashMap;

use crate::terrain::LoadedChunks;
use crate::systems::ClientWorldRoot;
use crate::states::GameState;
use shared::weapons::WeaponDebugMode;

/// Marker for environment prop entities
#[derive(Component)]
pub struct EnvironmentProp {
    pub chunk: ChunkCoord,
}

/// Stores which prop kind was spawned (used for collider debug / future collision).
#[derive(Component, Clone, Copy, Debug)]
pub struct PropKindTag(pub shared::PropKind);

/// Tracks which chunks have had props spawned
#[derive(Resource, Default)]
pub struct LoadedPropChunks {
    pub chunks: std::collections::HashSet<ChunkCoord>,
}

/// Handles to loaded prop assets
#[derive(Resource)]
pub struct PropAssets {
    pub scenes: HashMap<shared::PropKind, Handle<Scene>>,
}

/// Client-side derived collider info (for debug gizmos).
#[derive(Resource)]
pub struct ClientDerivedColliderLibrary {
    pub by_kind: HashMap<shared::PropKind, DerivedCollider>,
}

/// A face of the convex hull (triangle).
#[derive(Clone, Debug)]
pub struct HullFace {
    pub vertices: [Vec3; 3],
}

#[derive(Clone, Debug)]
pub struct DerivedCollider {
    pub bounding_radius: f32,
    /// Triangulated faces of the 3D convex hull
    pub hull_faces: Vec<HullFace>,
}

/// Plugin for environmental props
pub struct PropsPlugin;

impl Plugin for PropsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LoadedPropChunks>();
        app.add_systems(Startup, (load_prop_assets, load_baked_prop_colliders));
        app.add_systems(
            Update,
            (
                spawn_chunk_props,
                apply_prop_render_tuning,
                cleanup_chunk_props,
                debug_draw_prop_colliders,
            )
                .run_if(in_state(GameState::Playing)),
        );
    }
}

/// Load all prop GLTF assets at startup
fn load_prop_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    let mut scenes = HashMap::new();
    for kind in shared::ALL_PROP_KINDS.iter().copied() {
        scenes.insert(kind, asset_server.load(kind.scene_path()));
    }

    commands.insert_resource(PropAssets { scenes });

    info!("Loaded environmental prop assets");
}

/// Load baked colliders (for debug visualization).
fn load_baked_prop_colliders(mut commands: Commands) {
    let path = "client/assets/colliders.bin";
    let db = match shared::load_baked_collider_db_from_file(path) {
        Ok(db) => db,
        Err(e) => {
            warn!("Could not load baked colliders at {path}: {e} (debug gizmos disabled)");
            return;
        }
    };

    let mut by_kind = HashMap::new();
    for kind in shared::ALL_PROP_KINDS.iter().copied() {
        let Some(baked) = db.entries.get(kind.id()) else { continue };
        if let Some(d) = derive_collider(baked) {
            by_kind.insert(kind, d);
        }
    }

    info!("Loaded baked colliders for debug: {} kinds", by_kind.len());
    commands.insert_resource(ClientDerivedColliderLibrary { by_kind });
}

/// Spawn props for newly loaded terrain chunks
fn spawn_chunk_props(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    prop_assets: Option<Res<PropAssets>>,
    loaded_chunks: Res<LoadedChunks>,
    mut loaded_prop_chunks: ResMut<LoadedPropChunks>,
    world_root_query: Query<Entity, With<ClientWorldRoot>>,
) {
    let Some(assets) = prop_assets else { return };
    let Ok(world_root) = world_root_query.single() else { return };

    // Find chunks that need props
    for coord in loaded_chunks.chunks.iter() {
        if loaded_prop_chunks.chunks.contains(coord) {
            continue;
        }

        let spawns = shared::generate_chunk_prop_spawns(&terrain.generator, *coord);
        for spawn in spawns {
            let Some(scene) = assets.scenes.get(&spawn.kind).cloned() else {
                continue;
            };

            let prop = commands
                .spawn((
                    EnvironmentProp { chunk: spawn.chunk },
                    PropKindTag(spawn.kind),
                    spawn.render_tuning,
                    SceneRoot(scene),
                    Transform::from_translation(spawn.position)
                        .with_rotation(spawn.rotation)
                        .with_scale(Vec3::splat(spawn.scale)),
                ))
                .id();
            commands.entity(world_root).add_child(prop);
        }

        loaded_prop_chunks.chunks.insert(*coord);
    }
}

fn derive_collider(baked: &shared::BakedCollider) -> Option<DerivedCollider> {
    match baked {
        shared::BakedCollider::ConvexHull { points } => {
            if points.len() < 4 {
                return None;
            }
            
            let mut r2 = 0.0f32;
            let vertices: Vec<Vec3> = points.iter()
                .map(|p| {
                    r2 = r2.max(p[0] * p[0] + p[2] * p[2]);
                    Vec3::new(p[0], p[1], p[2])
                })
                .collect();
            
            // Triangulate the convex hull
            let hull_faces = triangulate_convex_hull(&vertices);
            
            Some(DerivedCollider {
                bounding_radius: r2.sqrt(),
                hull_faces,
            })
        }
    }
}

/// Triangulate a convex hull from its vertices for visualization.
fn triangulate_convex_hull(vertices: &[Vec3]) -> Vec<HullFace> {
    if vertices.len() < 4 {
        return vec![];
    }

    let centroid = vertices.iter().fold(Vec3::ZERO, |a, &b| a + b) / vertices.len() as f32;
    let mut faces = Vec::new();
    let n = vertices.len();
    
    for i in 0..n {
        for j in (i + 1)..n {
            for k in (j + 1)..n {
                let v0 = vertices[i];
                let v1 = vertices[j];
                let v2 = vertices[k];
                
                let e1 = v1 - v0;
                let e2 = v2 - v0;
                let mut normal = e1.cross(e2);
                let len = normal.length();
                if len < 1e-6 {
                    continue;
                }
                normal /= len;
                
                let d = normal.dot(v0);
                
                // Check if all other vertices are behind this plane
                let mut valid = true;
                for m in 0..n {
                    if m == i || m == j || m == k {
                        continue;
                    }
                    let dist = normal.dot(vertices[m]) - d;
                    if dist > 1e-4 {
                        valid = false;
                        break;
                    }
                }
                
                if !valid {
                    // Try flipped normal
                    let flipped_normal = -normal;
                    let flipped_d = -d;
                    
                    let mut valid_flipped = true;
                    for m in 0..n {
                        if m == i || m == j || m == k {
                            continue;
                        }
                        let dist = flipped_normal.dot(vertices[m]) - flipped_d;
                        if dist > 1e-4 {
                            valid_flipped = false;
                            break;
                        }
                    }
                    
                    if valid_flipped {
                        faces.push(HullFace {
                            vertices: [v0, v2, v1],
                        });
                    }
                } else {
                    // Ensure outward-facing
                    let to_centroid = centroid - v0;
                    if normal.dot(to_centroid) > 0.0 {
                        faces.push(HullFace {
                            vertices: [v0, v2, v1],
                        });
                    } else {
                        faces.push(HullFace {
                            vertices: [v0, v1, v2],
                        });
                    }
                }
            }
        }
    }
    
    // Remove duplicates
    faces.dedup_by(|a, b| {
        let mut a_verts: Vec<_> = a.vertices.iter().map(|v| 
            ((v.x * 1000.0) as i32, (v.y * 1000.0) as i32, (v.z * 1000.0) as i32)
        ).collect();
        let mut b_verts: Vec<_> = b.vertices.iter().map(|v| 
            ((v.x * 1000.0) as i32, (v.y * 1000.0) as i32, (v.z * 1000.0) as i32)
        ).collect();
        a_verts.sort();
        b_verts.sort();
        a_verts == b_verts
    });
    
    faces
}

/// Draw client-side prop collider gizmos (debug-only).
fn debug_draw_prop_colliders(
    mut gizmos: Gizmos,
    debug_mode: Res<WeaponDebugMode>,
    library: Option<Res<ClientDerivedColliderLibrary>>,
    camera: Query<&Transform, With<Camera3d>>,
    props: Query<(&PropKindTag, &Transform), With<EnvironmentProp>>,
) {
    if !debug_mode.0 {
        return;
    }
    let Some(library) = library else { return };

    let Ok(camera) = camera.single() else { return };
    let cam_pos = camera.translation;
    let max_dist = 120.0;
    let max_dist2 = max_dist * max_dist;

    let color = Color::srgba(0.0, 1.0, 1.0, 0.5);

    for (kind, transform) in props.iter() {
        let Some(shape) = library.by_kind.get(&kind.0) else { continue };

        let pos = transform.translation;
        if (pos - cam_pos).length_squared() > max_dist2 {
            continue;
        }

        let s = transform.scale.x;

        if !shape.hull_faces.is_empty() {
            // Draw actual 3D convex hull wireframe with rotation applied
            let rot = transform.rotation;
            for face in &shape.hull_faces {
                // Apply rotation then scale, then translate
                let v0 = pos + rot * (face.vertices[0] * s);
                let v1 = pos + rot * (face.vertices[1] * s);
                let v2 = pos + rot * (face.vertices[2] * s);
                
                gizmos.line(v0, v1, color);
                gizmos.line(v1, v2, color);
                gizmos.line(v2, v0, color);
            }
        } else {
            // Fallback: draw bounding sphere
            let circle_rot = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
            let r = (shape.bounding_radius * s).max(0.05);
            let iso = Isometry3d::new(pos, circle_rot);
            gizmos.circle(iso, r, color).resolution(24);
        }
    }
}

/// Apply [`PropRenderTuning`] to newly spawned meshes under prop scene hierarchies.
fn apply_prop_render_tuning(
    mut commands: Commands,
    new_meshes: Query<Entity, Added<Mesh3d>>,
    parents: Query<&ChildOf>,
    tunings: Query<&PropRenderTuning>,
) {
    for mesh_entity in new_meshes.iter() {
        // Walk up the hierarchy until we find an ancestor with `PropRenderTuning`.
        let mut current = mesh_entity;
        let tuning = loop {
            if let Ok(tuning) = tunings.get(current) {
                break Some(*tuning);
            }
            let Ok(parent) = parents.get(current) else {
                break None;
            };
            current = parent.parent();
        };

        let Some(tuning) = tuning else { continue };

        if !tuning.casts_shadows {
            commands.entity(mesh_entity).insert(NotShadowCaster);
        }
        if let Some(end) = tuning.visible_end_distance {
            commands.entity(mesh_entity).insert(VisibilityRange::abrupt(0.0, end));
        }
    }
}

/// Clean up props when their chunk is unloaded
fn cleanup_chunk_props(
    mut commands: Commands,
    loaded_chunks: Res<LoadedChunks>,
    mut loaded_prop_chunks: ResMut<LoadedPropChunks>,
    props: Query<(Entity, &EnvironmentProp)>,
) {
    // Find chunks that are no longer loaded
    let chunks_to_remove: Vec<ChunkCoord> = loaded_prop_chunks
        .chunks
        .difference(&loaded_chunks.chunks)
        .cloned()
        .collect();

    for coord in chunks_to_remove {
        // Despawn all props in this chunk
        for (entity, prop) in props.iter() {
            if prop.chunk == coord {
                commands.entity(entity).despawn();
            }
        }
        loaded_prop_chunks.chunks.remove(&coord);
    }
}
