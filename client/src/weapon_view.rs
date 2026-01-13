//! First-person weapon view and 3D weapon models
//!
//! Shows the currently equipped weapon in first-person view and handles weapon switching.
//! Updated for Lightyear 0.25 / Bevy 0.17

use bevy::prelude::*;
use lightyear::prelude::*;
use shared::{
    weapons::WeaponType, EquippedWeapon, LocalPlayer, SelectHotbarSlot, HotbarSelection, ReliableChannel,
};

use crate::input::{CameraMode, InputState};

/// Marker for the first-person weapon model
#[derive(Component)]
pub struct FirstPersonWeapon;

/// Marker for the third-person weapon model (attached to player)
#[derive(Component)]
pub struct ThirdPersonWeapon;

/// Track which weapon the third-person model is showing
#[derive(Resource, Default)]
pub struct CurrentThirdPersonWeapon {
    pub weapon_type: Option<WeaponType>,
}

/// Marker for weapon HUD root
#[derive(Component)]
pub struct WeaponHUD;

/// Marker for ammo text
#[derive(Component)]
pub struct AmmoText;

/// Marker for weapon name text
#[derive(Component)]
pub struct WeaponNameText;

/// Marker for hotbar slots display
#[derive(Component)]
pub struct HotbarSlotsText;

/// Resource tracking which weapon model is currently shown
#[derive(Resource, Default)]
pub struct CurrentWeaponView {
    pub weapon_type: Option<WeaponType>,
}

/// Handle weapon switching with number keys
pub fn handle_weapon_switch(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut client_query: Query<&mut MessageSender<SelectHotbarSlot>, (With<crate::GameClient>, With<Connected>)>,
    mut local_hotbar: Query<&mut HotbarSelection, With<LocalPlayer>>,
    input_state: Res<InputState>,
) {
    // Don't switch weapons while in vehicle
    if input_state.in_vehicle {
        return;
    }
    
    let new_index: Option<u8> = if keyboard.just_pressed(KeyCode::Digit1) {
        Some(0)
    } else if keyboard.just_pressed(KeyCode::Digit2) {
        Some(1)
    } else if keyboard.just_pressed(KeyCode::Digit3) {
        Some(2)
    } else if keyboard.just_pressed(KeyCode::Digit4) {
        Some(3)
    } else if keyboard.just_pressed(KeyCode::Digit5) {
        Some(4)
    } else if keyboard.just_pressed(KeyCode::Digit6) {
        Some(5)
    } else {
        None
    };
    
    let Some(index) = new_index else { return };
    
    // Optimistic local update (UI highlight feels instant)
    if let Ok(mut hotbar) = local_hotbar.single_mut() {
        hotbar.index = index;
    }
    
    if let Ok(mut sender) = client_query.single_mut() {
        let _ = sender.send::<ReliableChannel>(SelectHotbarSlot { index });
    }
}

/// Spawn the weapon HUD
pub fn spawn_weapon_hud(mut commands: Commands) {
    // HUD container at bottom-right
    commands
        .spawn((
            WeaponHUD,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(20.0),
                bottom: Val::Px(20.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::End,
                row_gap: Val::Px(5.0),
                ..default()
            },
        ))
        .with_children(|parent| {
            // Weapon name
            parent.spawn((
                WeaponNameText,
                Text::new("Assault Rifle"),
                TextFont {
                    font_size: 24.0,
                    ..default()
                },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
            ));
            
            // Ammo counter
            parent.spawn((
                AmmoText,
                Text::new("30 / 90"),
                TextFont {
                    font_size: 32.0,
                    ..default()
                },
                TextColor(Color::srgba(1.0, 0.9, 0.6, 1.0)),
            ));
            
            // Hotbar slots display (dynamically updated)
            parent.spawn((
                HotbarSlotsText,
                Text::new("[1] -  [2] -  [3] -  [4] -  [5] -  [6] -"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgba(0.7, 0.7, 0.7, 0.6)),
            ));
        });
}

/// Despawn weapon HUD and first-person weapon model when leaving gameplay
pub fn despawn_weapon_hud(
    mut commands: Commands,
    hud: Query<Entity, With<WeaponHUD>>,
    weapon_models: Query<Entity, With<FirstPersonWeapon>>,
    mut current_view: ResMut<CurrentWeaponView>,
) {
    for entity in hud.iter() {
        commands.entity(entity).despawn();
    }
    for entity in weapon_models.iter() {
        commands.entity(entity).despawn();
    }
    // Reset the weapon view state
    current_view.weapon_type = None;
}

/// Update HUD to show current weapon and ammo
pub fn update_weapon_hud(
    local_player: Query<(&EquippedWeapon, &shared::Inventory, &HotbarSelection), With<LocalPlayer>>,
    mut weapon_text: Query<&mut Text, (With<WeaponNameText>, Without<AmmoText>, Without<HotbarSlotsText>)>,
    mut ammo_text: Query<&mut Text, (With<AmmoText>, Without<WeaponNameText>, Without<HotbarSlotsText>)>,
    mut hotbar_text: Query<&mut Text, (With<HotbarSlotsText>, Without<WeaponNameText>, Without<AmmoText>)>,
    mut hud_visibility: Query<&mut Visibility, With<WeaponHUD>>,
    input_state: Res<InputState>,
) {
    // Hide HUD in vehicle
    for mut vis in hud_visibility.iter_mut() {
        *vis = if input_state.in_vehicle {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }
    
    let Some((weapon, inventory, hotbar_selection)) = local_player.iter().next() else {
        return;
    };
    
    // Update weapon name
    for mut text in weapon_text.iter_mut() {
        **text = weapon_name(weapon.weapon_type);
    }
    
    // Update hotbar slots display
    for mut text in hotbar_text.iter_mut() {
        let mut slots = Vec::new();
        for i in 0..shared::HOTBAR_SLOTS {
            let slot_name = if let Some(stack) = inventory.get_slot(i) {
                hotbar_item_name(&stack.item_type)
            } else {
                "-".to_string()
            };
            // Highlight active slot
            if i == hotbar_selection.index as usize {
                slots.push(format!(">[{}] {}<", i + 1, slot_name));
            } else {
                slots.push(format!("[{}] {}", i + 1, slot_name));
            }
        }
        **text = slots.join("  ");
    }
    
    // Update ammo - reserve comes from inventory (no ammo display when Unarmed)
    if weapon.weapon_type == WeaponType::Unarmed {
        for mut text in ammo_text.iter_mut() {
            **text = String::new();
        }
        return;
    }
    
    let reserve_ammo = inventory.count_item(weapon.weapon_type.ammo_type());
    for mut text in ammo_text.iter_mut() {
        **text = format!("{} / {}", weapon.ammo_in_mag, reserve_ammo);
    }
}

/// Get display name for weapon type
fn weapon_name(weapon: WeaponType) -> String {
    match weapon {
        WeaponType::Pistol => "Pistol".to_string(),
        WeaponType::AssaultRifle => "Assault Rifle".to_string(),
        WeaponType::Sniper => "Sniper Rifle".to_string(),
        WeaponType::Shotgun => "Shotgun".to_string(),
        WeaponType::SMG => "SMG".to_string(),
        WeaponType::Unarmed => "Unarmed".to_string(),
    }
}

/// Get short display name for hotbar items
fn hotbar_item_name(item_type: &shared::ItemType) -> String {
    use shared::ItemType;
    match item_type {
        ItemType::Weapon(w) => match w {
            WeaponType::Pistol => "Pistol".to_string(),
            WeaponType::AssaultRifle => "AR".to_string(),
            WeaponType::Sniper => "Sniper".to_string(),
            WeaponType::Shotgun => "Shotgun".to_string(),
            WeaponType::SMG => "SMG".to_string(),
            WeaponType::Unarmed => "-".to_string(),
        },
        ItemType::PistolAmmo => "9mm".to_string(),
        ItemType::RifleAmmo => "5.56".to_string(),
        ItemType::ShotgunShells => "Shells".to_string(),
        ItemType::SniperRounds => ".308".to_string(),
        ItemType::Wood => "Wood".to_string(),
        ItemType::Stone => "Stone".to_string(),
    }
}

/// Spawn or update the first-person weapon model
pub fn update_first_person_weapon(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    local_player: Query<&EquippedWeapon, With<LocalPlayer>>,
    camera: Query<Entity, With<Camera3d>>,
    existing_weapon: Query<Entity, With<FirstPersonWeapon>>,
    mut current_view: ResMut<CurrentWeaponView>,
    input_state: Res<InputState>,
) {
    let Some(weapon) = local_player.iter().next() else {
        return;
    };
    
    let Some(camera_entity) = camera.iter().next() else {
        return;
    };
    
    // Hide weapon in third-person or vehicle
    let should_show = input_state.camera_mode == CameraMode::FirstPerson
        && !input_state.in_vehicle
        && weapon.weapon_type != WeaponType::Unarmed;
    
    // Check if we need to change the model
    let needs_update = current_view.weapon_type != Some(weapon.weapon_type);
    
    // Despawn old weapon if changing or hiding
    if needs_update || !should_show {
        for entity in existing_weapon.iter() {
            commands.entity(entity).despawn();
        }
        current_view.weapon_type = None;
    }
    
    if !should_show {
        return;
    }
    
    // Spawn new weapon model if needed
    if needs_update {
        spawn_weapon_model(
            &mut commands,
            &mut meshes,
            &mut materials,
            weapon.weapon_type,
            camera_entity,
        );
        current_view.weapon_type = Some(weapon.weapon_type);
    }
}

/// Spawn the 3D weapon model attached to the camera
fn spawn_weapon_model(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    weapon_type: WeaponType,
    camera_entity: Entity,
) {
    // Weapon materials
    let metal_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.15, 0.18),
        metallic: 0.9,
        perceptual_roughness: 0.3,
        ..default()
    });
    
    let grip_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.06, 0.04),
        metallic: 0.1,
        perceptual_roughness: 0.8,
        ..default()
    });
    
    let accent_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.55, 0.4),
        metallic: 0.7,
        perceptual_roughness: 0.4,
        ..default()
    });

    // Position in bottom-right of view (camera-relative)
    let base_offset = Vec3::new(0.25, -0.2, -0.5);
    
    let weapon_entity = commands.spawn((
        FirstPersonWeapon,
        Transform::from_translation(base_offset),
        // Explicit GlobalTransform avoids B0004 warnings when child meshes are spawned immediately.
        GlobalTransform::default(),
        Visibility::Inherited,
        InheritedVisibility::default(),
    )).id();
    
    // Build weapon based on type
    match weapon_type {
        WeaponType::Pistol => {
            // Compact pistol
            let body = meshes.add(Cuboid::new(0.03, 0.08, 0.12));
            let barrel = meshes.add(Cylinder::new(0.012, 0.08));
            let grip = meshes.add(Cuboid::new(0.025, 0.07, 0.03));
            
            commands.entity(weapon_entity).with_children(|parent| {
                // Main body
                parent.spawn((
                    Mesh3d(body),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                ));
                // Barrel
                parent.spawn((
                    Mesh3d(barrel),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.02, -0.08))
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                ));
                // Grip
                parent.spawn((
                    Mesh3d(grip),
                    MeshMaterial3d(grip_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, -0.06, 0.02))
                        .with_rotation(Quat::from_rotation_x(-0.2)),
                ));
            });
        }
        
        WeaponType::AssaultRifle => {
            // AR with longer body
            let body = meshes.add(Cuboid::new(0.04, 0.06, 0.35));
            let barrel = meshes.add(Cylinder::new(0.012, 0.15));
            let magazine = meshes.add(Cuboid::new(0.02, 0.08, 0.025));
            let stock = meshes.add(Cuboid::new(0.03, 0.05, 0.12));
            let grip = meshes.add(Cuboid::new(0.025, 0.06, 0.03));
            let sight = meshes.add(Cuboid::new(0.015, 0.025, 0.04));
            
            commands.entity(weapon_entity).with_children(|parent| {
                // Main body
                parent.spawn((
                    Mesh3d(body),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                ));
                // Barrel
                parent.spawn((
                    Mesh3d(barrel),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.25))
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                ));
                // Magazine
                parent.spawn((
                    Mesh3d(magazine),
                    MeshMaterial3d(accent_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, -0.07, 0.0))
                        .with_rotation(Quat::from_rotation_x(-0.15)),
                ));
                // Stock
                parent.spawn((
                    Mesh3d(stock),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, 0.22)),
                ));
                // Grip
                parent.spawn((
                    Mesh3d(grip),
                    MeshMaterial3d(grip_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, -0.05, 0.08))
                        .with_rotation(Quat::from_rotation_x(-0.25)),
                ));
                // Iron sight
                parent.spawn((
                    Mesh3d(sight),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.045, -0.05)),
                ));
            });
        }
        
        WeaponType::Sniper => {
            // Long sniper rifle
            let body = meshes.add(Cuboid::new(0.035, 0.055, 0.5));
            let barrel = meshes.add(Cylinder::new(0.015, 0.3));
            let scope = meshes.add(Cylinder::new(0.02, 0.12));
            let stock = meshes.add(Cuboid::new(0.03, 0.06, 0.18));
            let grip = meshes.add(Cuboid::new(0.025, 0.06, 0.03));
            let bipod_leg = meshes.add(Cylinder::new(0.005, 0.08));
            
            commands.entity(weapon_entity).with_children(|parent| {
                // Main body
                parent.spawn((
                    Mesh3d(body),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                ));
                // Long barrel
                parent.spawn((
                    Mesh3d(barrel),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.38))
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                ));
                // Scope
                parent.spawn((
                    Mesh3d(scope),
                    MeshMaterial3d(accent_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.055, -0.05))
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                ));
                // Stock
                parent.spawn((
                    Mesh3d(stock),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, -0.01, 0.32)),
                ));
                // Grip
                parent.spawn((
                    Mesh3d(grip),
                    MeshMaterial3d(grip_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, -0.05, 0.12))
                        .with_rotation(Quat::from_rotation_x(-0.2)),
                ));
                // Bipod legs
                parent.spawn((
                    Mesh3d(bipod_leg.clone()),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.02, -0.06, -0.2))
                        .with_rotation(Quat::from_rotation_x(0.3)),
                ));
                parent.spawn((
                    Mesh3d(bipod_leg),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(-0.02, -0.06, -0.2))
                        .with_rotation(Quat::from_rotation_x(0.3)),
                ));
            });
        }
        
        WeaponType::Shotgun => {
            // Pump-action shotgun
            let body = meshes.add(Cuboid::new(0.04, 0.05, 0.35));
            let barrel = meshes.add(Cylinder::new(0.018, 0.25));
            let pump = meshes.add(Cuboid::new(0.035, 0.04, 0.08));
            let stock = meshes.add(Cuboid::new(0.035, 0.055, 0.15));
            let grip = meshes.add(Cuboid::new(0.03, 0.055, 0.035));
            
            commands.entity(weapon_entity).with_children(|parent| {
                // Main body/receiver
                parent.spawn((
                    Mesh3d(body),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                ));
                // Barrel
                parent.spawn((
                    Mesh3d(barrel),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.28))
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                ));
                // Pump/forend
                parent.spawn((
                    Mesh3d(pump),
                    MeshMaterial3d(grip_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, -0.01, -0.12)),
                ));
                // Stock
                parent.spawn((
                    Mesh3d(stock),
                    MeshMaterial3d(grip_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, 0.23)),
                ));
                // Grip
                parent.spawn((
                    Mesh3d(grip),
                    MeshMaterial3d(grip_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, -0.045, 0.08))
                        .with_rotation(Quat::from_rotation_x(-0.25)),
                ));
            });
        }
        
        WeaponType::SMG => {
            // Compact SMG
            let body = meshes.add(Cuboid::new(0.035, 0.055, 0.2));
            let barrel = meshes.add(Cylinder::new(0.01, 0.08));
            let magazine = meshes.add(Cuboid::new(0.02, 0.1, 0.02));
            let grip = meshes.add(Cuboid::new(0.025, 0.055, 0.028));
            let foregrip = meshes.add(Cuboid::new(0.02, 0.04, 0.025));
            let stock = meshes.add(Cuboid::new(0.015, 0.025, 0.08));
            
            commands.entity(weapon_entity).with_children(|parent| {
                // Main body
                parent.spawn((
                    Mesh3d(body),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, 0.0)),
                ));
                // Barrel
                parent.spawn((
                    Mesh3d(barrel),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.14))
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                ));
                // Long magazine
                parent.spawn((
                    Mesh3d(magazine),
                    MeshMaterial3d(accent_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, -0.075, -0.02)),
                ));
                // Grip
                parent.spawn((
                    Mesh3d(grip),
                    MeshMaterial3d(grip_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, -0.045, 0.05))
                        .with_rotation(Quat::from_rotation_x(-0.2)),
                ));
                // Foregrip
                parent.spawn((
                    Mesh3d(foregrip),
                    MeshMaterial3d(grip_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, -0.045, -0.06)),
                ));
                // Folding stock
                parent.spawn((
                    Mesh3d(stock),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.015, 0.14)),
                ));
            });
        }
        
        WeaponType::Unarmed => {
            // No model
        }
    }
    
    // Make weapon a child of camera so it follows view
    commands.entity(camera_entity).add_child(weapon_entity);
}

/// Add slight weapon sway/bob for visual polish
pub fn animate_weapon(
    mut weapons: Query<&mut Transform, With<FirstPersonWeapon>>,
    input_state: Res<InputState>,
    time: Res<Time>,
) {
    let t = time.elapsed_secs();
    
    for mut transform in weapons.iter_mut() {
        // Base position
        let mut offset = Vec3::new(0.25, -0.2, -0.5);
        
        // Subtle breathing/idle sway
        offset.x += (t * 1.2).sin() * 0.003;
        offset.y += (t * 0.8).cos() * 0.002;
        
        // Movement bob (if walking)
        if input_state.forward || input_state.backward || input_state.left || input_state.right {
            offset.y += (t * 8.0).sin().abs() * 0.008;
            offset.x += (t * 4.0).sin() * 0.004;
        }
        
        transform.translation = offset;
    }
}

// =============================================================================
// THIRD-PERSON WEAPON
// =============================================================================

/// Update third-person weapon model (visible on character in third-person view)
pub fn update_third_person_weapon(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    local_player: Query<(Entity, &EquippedWeapon), With<LocalPlayer>>,
    existing_weapon: Query<Entity, With<ThirdPersonWeapon>>,
    mut current_tp_weapon: ResMut<CurrentThirdPersonWeapon>,
    input_state: Res<InputState>,
) {
    let Some((player_entity, weapon)) = local_player.iter().next() else {
        return;
    };
    
    // Only show in third-person and not in vehicle
    let should_show = input_state.camera_mode == CameraMode::ThirdPerson
        && !input_state.in_vehicle
        && weapon.weapon_type != WeaponType::Unarmed;
    
    // Check if we need to spawn (not already showing this weapon)
    let already_showing = current_tp_weapon.weapon_type == Some(weapon.weapon_type);
    
    // Despawn old weapon if switching weapons or hiding
    if !should_show || (should_show && !already_showing) {
        for entity in existing_weapon.iter() {
            commands.entity(entity).despawn();
        }
        current_tp_weapon.weapon_type = None;
    }
    
    if !should_show {
        return;
    }
    
    // Spawn new weapon model if not already showing
    if current_tp_weapon.weapon_type.is_none() {
        info!("Spawning third-person weapon: {:?}", weapon.weapon_type);
        spawn_third_person_weapon(
            &mut commands,
            &mut meshes,
            &mut materials,
            weapon.weapon_type,
            player_entity,
        );
        current_tp_weapon.weapon_type = Some(weapon.weapon_type);
    }
}

/// Spawn a simplified third-person weapon model attached to the player
fn spawn_third_person_weapon(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    weapon_type: WeaponType,
    player_entity: Entity,
) {
    // Weapon materials (same as first-person but we can adjust)
    let metal_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.15, 0.18),
        metallic: 0.9,
        perceptual_roughness: 0.3,
        ..default()
    });
    
    let grip_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.06, 0.04),
        metallic: 0.1,
        perceptual_roughness: 0.8,
        ..default()
    });

    // Position relative to player - roughly where hands would hold a weapon
    // Offset: slightly forward, to the right, and at chest height
    let base_offset = Vec3::new(0.2, 0.15, -0.35);
    
    let weapon_entity = commands.spawn((
        ThirdPersonWeapon,
        Transform::from_translation(base_offset)
            .with_rotation(Quat::from_rotation_y(-0.1)), // Slight angle
        GlobalTransform::default(),
        Visibility::Inherited,
        InheritedVisibility::default(),
    )).id();
    
    // Build simplified weapon models (less detail than first-person)
    let scale = 0.8; // Slightly smaller for third person
    
    match weapon_type {
        WeaponType::Pistol => {
            let body = meshes.add(Cuboid::new(0.04 * scale, 0.08 * scale, 0.12 * scale));
            let barrel = meshes.add(Cylinder::new(0.012 * scale, 0.08 * scale));
            
            commands.entity(weapon_entity).with_children(|parent| {
                parent.spawn((
                    Mesh3d(body),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::default(),
                ));
                parent.spawn((
                    Mesh3d(barrel),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.02 * scale, -0.1 * scale))
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                ));
            });
        }
        WeaponType::AssaultRifle | WeaponType::SMG => {
            let body = meshes.add(Cuboid::new(0.04 * scale, 0.06 * scale, 0.35 * scale));
            let barrel = meshes.add(Cylinder::new(0.012 * scale, 0.15 * scale));
            let mag = meshes.add(Cuboid::new(0.025 * scale, 0.08 * scale, 0.04 * scale));
            
            commands.entity(weapon_entity).with_children(|parent| {
                parent.spawn((
                    Mesh3d(body),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::default(),
                ));
                parent.spawn((
                    Mesh3d(barrel),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.25 * scale))
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                ));
                parent.spawn((
                    Mesh3d(mag),
                    MeshMaterial3d(grip_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, -0.06 * scale, 0.05 * scale)),
                ));
            });
        }
        WeaponType::Shotgun => {
            let body = meshes.add(Cuboid::new(0.045 * scale, 0.06 * scale, 0.4 * scale));
            let barrel = meshes.add(Cylinder::new(0.018 * scale, 0.25 * scale));
            
            commands.entity(weapon_entity).with_children(|parent| {
                parent.spawn((
                    Mesh3d(body),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::default(),
                ));
                parent.spawn((
                    Mesh3d(barrel),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.01 * scale, -0.3 * scale))
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                ));
            });
        }
        WeaponType::Sniper => {
            let body = meshes.add(Cuboid::new(0.04 * scale, 0.06 * scale, 0.5 * scale));
            let barrel = meshes.add(Cylinder::new(0.015 * scale, 0.3 * scale));
            let scope = meshes.add(Cylinder::new(0.02 * scale, 0.12 * scale));
            
            commands.entity(weapon_entity).with_children(|parent| {
                parent.spawn((
                    Mesh3d(body),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::default(),
                ));
                parent.spawn((
                    Mesh3d(barrel),
                    MeshMaterial3d(metal_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.0, -0.4 * scale))
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                ));
                parent.spawn((
                    Mesh3d(scope),
                    MeshMaterial3d(grip_material.clone()),
                    Transform::from_translation(Vec3::new(0.0, 0.05 * scale, -0.05 * scale))
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
                ));
            });
        }
        WeaponType::Unarmed => {
            // No model
        }
    }
    
    // Make weapon a child of the player so it follows them
    commands.entity(player_entity).add_child(weapon_entity);
}

/// Despawn third-person weapon when leaving gameplay
pub fn despawn_third_person_weapon(
    mut commands: Commands,
    weapons: Query<Entity, With<ThirdPersonWeapon>>,
    mut current_tp_weapon: ResMut<CurrentThirdPersonWeapon>,
) {
    for entity in weapons.iter() {
        commands.entity(entity).despawn();
    }
    current_tp_weapon.weapon_type = None;
}
