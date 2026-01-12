//! Client-side NPC visuals, animation, and debug rendering.

use bevy::animation::{AnimationClip, AnimationPlayer, AnimationTarget, AnimationTargetId};
use bevy::animation::graph::{AnimationGraph, AnimationGraphHandle, AnimationNodeIndex};
use bevy::prelude::*;

use shared::{
    npc_capsule_endpoints, npc_head_center, Npc, NpcArchetype, NpcPosition, NpcRotation, Health,
    NPC_HEAD_RADIUS, NPC_HEIGHT, NPC_RADIUS,
};

use shared::WeaponDebugMode;

// =============================================================================
// ASSETS
// =============================================================================

#[derive(Resource, Clone)]
pub struct KayKitNpcAssets {
    pub scenes: std::collections::HashMap<NpcArchetype, Handle<Scene>>,
    pub animation_graph: Handle<AnimationGraph>,
    pub idle_node: AnimationNodeIndex,
    pub walk_node: AnimationNodeIndex,
    pub death_node: AnimationNodeIndex,
}

pub fn setup_npc_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut animation_graphs: ResMut<Assets<AnimationGraph>>,
) {
    let mut scenes = std::collections::HashMap::new();

    let mut load_scene = |arch: NpcArchetype, name: &str| {
        let path = format!("KayKit_Adventurers_2.0_FREE/Characters/gltf/{name}.glb#Scene0");
        scenes.insert(arch, asset_server.load(path));
    };

    load_scene(NpcArchetype::Barbarian, "Barbarian");
    load_scene(NpcArchetype::Ranger, "Ranger");
    load_scene(NpcArchetype::Mage, "Mage");
    load_scene(NpcArchetype::Knight, "Knight");
    load_scene(NpcArchetype::Rogue, "Rogue");
    load_scene(NpcArchetype::RogueHooded, "Rogue_Hooded");

    // Animations (rig clips)
    // - Idle_A: Rig_Medium_General.glb#Animation6
    // - Walking_A: Rig_Medium_MovementBasic.glb#Animation8
    // - Death_A: Rig_Medium_General.glb#Animation0
    let idle_clip: Handle<AnimationClip> = asset_server.load(
        "KayKit_Adventurers_2.0_FREE/Animations/gltf/Rig_Medium/Rig_Medium_General.glb#Animation6",
    );
    let walk_clip: Handle<AnimationClip> = asset_server.load(
        "KayKit_Adventurers_2.0_FREE/Animations/gltf/Rig_Medium/Rig_Medium_MovementBasic.glb#Animation8",
    );
    let death_clip: Handle<AnimationClip> = asset_server.load(
        "KayKit_Adventurers_2.0_FREE/Animations/gltf/Rig_Medium/Rig_Medium_General.glb#Animation0",
    );

    let (graph, nodes) = AnimationGraph::from_clips([idle_clip, walk_clip, death_clip]);
    let animation_graph = animation_graphs.add(graph);
    let idle_node = nodes[0];
    let walk_node = nodes[1];
    let death_node = nodes[2];

    commands.insert_resource(KayKitNpcAssets {
        scenes,
        animation_graph,
        idle_node,
        walk_node,
        death_node,
    });

    info!("Loaded KayKit NPC assets (scenes + idle/walk/death animations)");
}

// =============================================================================
// SPAWNING
// =============================================================================

#[derive(Component)]
pub struct NpcModelRoot;

#[derive(Component)]
pub struct NeedsNpcRigSetup;

#[derive(Component)]
pub struct NpcAnimationRoot;

/// The NPC entity that owns this rig (cached to avoid per-frame hierarchy walks).
#[derive(Component, Clone, Copy)]
pub struct NpcRigOwner(pub Entity);

#[derive(Component, Default)]
pub struct NpcAnimState {
    pub walking: bool,
    pub dead: bool,
    pub initialized: bool,
    pub last_pos: Vec3,
}

/// Add render components and spawn the visual model when an NPC replicates in.
pub fn handle_npc_spawned(
    mut commands: Commands,
    assets: Option<Res<KayKitNpcAssets>>,
    new_npcs: Query<(Entity, &Npc, &NpcPosition), Added<Npc>>,
) {
    let Some(assets) = assets else { return };

    for (entity, npc, pos) in new_npcs.iter() {
        // Ensure NPC entity has full spatial components for hierarchy propagation.
        // Without GlobalTransform, children with GlobalTransform trigger B0004 warnings.
        commands.entity(entity).insert((
            Transform::from_translation(pos.0),
            GlobalTransform::from_translation(pos.0),
            Visibility::Inherited,
            InheritedVisibility::default(),
        ));

        let Some(scene) = assets.scenes.get(&npc.archetype).cloned() else {
            warn!("No KayKit scene for NPC archetype {:?}; skipping model spawn", npc.archetype);
            continue;
        };

        commands.entity(entity).with_children(|parent| {
            parent.spawn((
                NpcModelRoot,
                NeedsNpcRigSetup,
                SceneRoot(scene),
                // NPC transform is the capsule center; drop model so feet touch ground.
                Transform::from_xyz(0.0, -NPC_HEIGHT * 0.5, 0.0)
                    .with_rotation(Quat::from_rotation_y(std::f32::consts::PI))
                    .with_scale(Vec3::splat(1.0)),
                GlobalTransform::default(),
                Visibility::Inherited,
                InheritedVisibility::default(),
            ));
        });
    }
}

/// Add `AnimationPlayer` + `AnimationTarget`s to the spawned KayKit NPC hierarchy.
pub fn setup_npc_rig(
    mut commands: Commands,
    assets: Option<Res<KayKitNpcAssets>>,
    model_roots: Query<Entity, (With<NpcModelRoot>, With<NeedsNpcRigSetup>)>,
    children_q: Query<&Children>,
    names_q: Query<&Name>,
    parents_q: Query<&ChildOf>,
) {
    let Some(assets) = assets else { return };

    for model_root in model_roots.iter() {
        // Cache the owning NPC entity (parent of NpcModelRoot).
        let Ok(owner) = parents_q.get(model_root).map(|p| p.parent()) else {
            continue;
        };

        // Find the rig root node inside the spawned scene (named "Rig_Medium" in KayKit).
        let mut stack: Vec<Entity> = vec![model_root];
        let mut rig_root: Option<Entity> = None;
        while let Some(e) = stack.pop() {
            if let Ok(name) = names_q.get(e) {
                if name.as_str() == "Rig_Medium" {
                    rig_root = Some(e);
                    break;
                }
            }
            if let Ok(children) = children_q.get(e) {
                stack.extend(children.iter());
            }
        }

        let Some(rig_root) = rig_root else {
            // Scene not spawned yet.
            continue;
        };

        commands.entity(rig_root).insert((
            NpcAnimationRoot,
            NpcRigOwner(owner),
            NpcAnimState::default(),
            AnimationPlayer::default(),
            AnimationGraphHandle(assets.animation_graph.clone()),
        ));

        // Generate AnimationTargets for the entire rig hierarchy.
        let Ok(root_name) = names_q.get(rig_root) else {
            commands.entity(model_root).remove::<NeedsNpcRigSetup>();
            continue;
        };

        let mut stack: Vec<(Entity, Vec<Name>)> = vec![(rig_root, vec![root_name.clone()])];
        while let Some((e, path)) = stack.pop() {
            commands.entity(e).insert(AnimationTarget {
                id: AnimationTargetId::from_names(path.iter()),
                player: rig_root,
            });

            if let Ok(children) = children_q.get(e) {
                for child in children.iter() {
                    let mut child_path = path.clone();
                    if let Ok(child_name) = names_q.get(child) {
                        child_path.push(child_name.clone());
                    }
                    stack.push((child, child_path));
                }
            }
        }

        commands.entity(model_root).remove::<NeedsNpcRigSetup>();
    }
}

// =============================================================================
// TRANSFORM SYNC
// =============================================================================

/// Sync NPC transforms from replicated components.
pub fn sync_npc_transforms(
    time: Res<Time>,
    mut npcs: Query<(&NpcPosition, &NpcRotation, &mut Transform), With<Npc>>,
) {
    let dt = time.delta_secs();
    let pos_rate: f32 = 18.0;
    let rot_rate: f32 = 22.0;
    let t_pos = 1.0_f32 - (-pos_rate * dt).exp();
    let t_rot = 1.0_f32 - (-rot_rate * dt).exp();

    for (pos, rot, mut transform) in npcs.iter_mut() {
        transform.translation = transform.translation.lerp(pos.0, t_pos);
        let target_rot = Quat::from_rotation_y(rot.0);
        transform.rotation = transform.rotation.slerp(target_rot, t_rot);
    }
}

// =============================================================================
// ANIMATION
// =============================================================================

pub fn update_npc_animation(
    assets: Option<Res<KayKitNpcAssets>>,
    time: Res<Time>,
    npc_roots: Query<(&Npc, &Health, &Transform)>,
    mut anim_roots: Query<(&NpcRigOwner, &mut NpcAnimState, &mut AnimationPlayer), With<NpcAnimationRoot>>,
) {
    let Some(assets) = assets else { return };

    let dt = time.delta_secs().max(1e-6);

    for (owner, mut state, mut player) in anim_roots.iter_mut() {
        let Ok((_npc, health, transform)) = npc_roots.get(owner.0) else {
            continue;
        };

        let health_current = health.current;
        let npc_pos = transform.translation;

        let is_dead = health_current <= 0.0;

        // Motion-based walking detection.
        let mut speed_xz = 0.0;
        if state.initialized {
            let d = npc_pos - state.last_pos;
            speed_xz = Vec2::new(d.x, d.z).length() / dt;
        } else {
            state.initialized = true;
        }
        state.last_pos = npc_pos;

        let should_walk = !is_dead && speed_xz > 0.15;

        if is_dead && !state.dead {
            player.stop_all();
            player.start(assets.death_node);
            state.dead = true;
            state.walking = false;
            continue;
        }

        if state.dead {
            // Keep death animation playing or finished; don't switch back.
            continue;
        }

        if should_walk && !state.walking {
            player.stop_all();
            player.start(assets.walk_node).repeat();
            state.walking = true;
        } else if !should_walk && state.walking {
            player.stop_all();
            player.start(assets.idle_node).repeat();
            state.walking = false;
        } else {
            // Ensure something is playing.
            let node = if state.walking { assets.walk_node } else { assets.idle_node };
            if !player.is_playing_animation(node) {
                player.start(node).repeat();
            }
        }
    }
}

// =============================================================================
// DEBUG (F3)
// =============================================================================

pub fn debug_draw_npc_hitboxes(
    mut gizmos: Gizmos,
    debug_mode: Res<WeaponDebugMode>,
    npcs: Query<(&Transform, &Health), With<Npc>>,
) {
    if !debug_mode.0 {
        return;
    }

    for (transform, health) in npcs.iter() {
        let center = transform.translation;

        let head_center = npc_head_center(center);
        let (a, b) = npc_capsule_endpoints(center);

        let alive = !health.is_dead();

        let body_color = if alive {
            Color::srgba(1.0, 0.85, 0.2, 0.9)
        } else {
            Color::srgba(0.6, 0.6, 0.6, 0.7)
        };
        let head_color = if alive {
            Color::srgba(1.0, 0.2, 0.2, 0.95)
        } else {
            Color::srgba(0.5, 0.2, 0.2, 0.7)
        };

        // Body capsule (approx): spheres at endpoints + line between.
        gizmos.sphere(Isometry3d::from_translation(a), NPC_RADIUS, body_color);
        gizmos.sphere(Isometry3d::from_translation(b), NPC_RADIUS, body_color);
        gizmos.line(a, b, body_color);

        // Head sphere.
        gizmos.sphere(Isometry3d::from_translation(head_center), NPC_HEAD_RADIUS, head_color);
    }
}
