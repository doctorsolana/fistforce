//! Crosshair UI for first-person shooting
//!
//! Simple centered dot crosshair that shows in first-person mode.
//! Shrinks when aiming down sights (ADS).

use bevy::prelude::*;
use crate::input::CameraMode;

/// Marker component for the crosshair UI
#[derive(Component)]
pub struct Crosshair;

/// Marker for the center dot
#[derive(Component)]
pub struct CrosshairDot;

/// Marker for crosshair lines (top/bottom/left/right)
#[derive(Component)]
pub struct CrosshairLine {
    /// Which direction this line points
    pub direction: CrosshairLineDir,
}

#[derive(Clone, Copy)]
pub enum CrosshairLineDir {
    Top,
    Bottom,
    Left,
    Right,
}

/// Marker for the hit marker overlay
#[derive(Component)]
pub struct HitMarker {
    pub spawn_time: f32,
    pub is_kill: bool,
}

/// Spawn the crosshair UI
pub fn spawn_crosshair(mut commands: Commands) {
    // Root container (full screen, centered)
    commands
        .spawn((
            Crosshair,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                position_type: PositionType::Absolute,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            // Ensure it doesn't block mouse input
            Pickable::IGNORE,
        ))
        .with_children(|parent| {
            // Center dot
            parent.spawn((
                CrosshairDot,
                Node {
                    width: Val::Px(4.0),
                    height: Val::Px(4.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
                BorderRadius::all(Val::Px(2.0)),
            ));
            
            // Top line
            parent.spawn((
                CrosshairLine { direction: CrosshairLineDir::Top },
                Node {
                    width: Val::Px(2.0),
                    height: Val::Px(8.0),
                    position_type: PositionType::Absolute,
                    top: Val::Px(-14.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
            ));
            
            // Bottom line
            parent.spawn((
                CrosshairLine { direction: CrosshairLineDir::Bottom },
                Node {
                    width: Val::Px(2.0),
                    height: Val::Px(8.0),
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(-14.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
            ));
            
            // Left line
            parent.spawn((
                CrosshairLine { direction: CrosshairLineDir::Left },
                Node {
                    width: Val::Px(8.0),
                    height: Val::Px(2.0),
                    position_type: PositionType::Absolute,
                    left: Val::Px(-14.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
            ));
            
            // Right line
            parent.spawn((
                CrosshairLine { direction: CrosshairLineDir::Right },
                Node {
                    width: Val::Px(8.0),
                    height: Val::Px(2.0),
                    position_type: PositionType::Absolute,
                    right: Val::Px(-14.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
            ));
        });
}

/// Update crosshair visibility based on camera mode
pub fn update_crosshair_visibility(
    mut crosshair_query: Query<&mut Visibility, With<Crosshair>>,
    input_state: Res<crate::input::InputState>,
) {
    for mut visibility in crosshair_query.iter_mut() {
        *visibility = match input_state.camera_mode {
            CameraMode::FirstPerson => Visibility::Visible,
            CameraMode::ThirdPerson => Visibility::Hidden,
        };
    }
}

/// Update crosshair appearance when aiming down sights
pub fn update_crosshair_ads(
    mut dot_query: Query<(&mut Node, &mut BackgroundColor), With<CrosshairDot>>,
    mut line_query: Query<(&CrosshairLine, &mut Node, &mut BackgroundColor), Without<CrosshairDot>>,
    input_state: Res<crate::input::InputState>,
) {
    let aiming = input_state.aiming;
    
    // Update center dot - smaller and more visible when aiming
    for (mut node, mut bg) in dot_query.iter_mut() {
        if aiming {
            node.width = Val::Px(3.0);
            node.height = Val::Px(3.0);
            *bg = BackgroundColor(Color::srgba(1.0, 0.3, 0.3, 1.0)); // Red dot when ADS
        } else {
            node.width = Val::Px(4.0);
            node.height = Val::Px(4.0);
            *bg = BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.85));
        }
    }
    
    // Update crosshair lines - move closer to center when aiming
    for (line, mut node, mut bg) in line_query.iter_mut() {
        let (hip_offset, ads_offset) = (14.0, 6.0); // Hip fire vs ADS offset from center
        let offset = if aiming { ads_offset } else { hip_offset };
        
        match line.direction {
            CrosshairLineDir::Top => {
                node.top = Val::Px(-offset);
                node.height = if aiming { Val::Px(6.0) } else { Val::Px(8.0) };
            }
            CrosshairLineDir::Bottom => {
                node.bottom = Val::Px(-offset);
                node.height = if aiming { Val::Px(6.0) } else { Val::Px(8.0) };
            }
            CrosshairLineDir::Left => {
                node.left = Val::Px(-offset);
                node.width = if aiming { Val::Px(6.0) } else { Val::Px(8.0) };
            }
            CrosshairLineDir::Right => {
                node.right = Val::Px(-offset);
                node.width = if aiming { Val::Px(6.0) } else { Val::Px(8.0) };
            }
        }
        
        // Change color when aiming
        if aiming {
            *bg = BackgroundColor(Color::srgba(1.0, 0.4, 0.4, 0.9)); // Reddish when ADS
        } else {
            *bg = BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.7));
        }
    }
}

/// Spawn a hit marker when we hit someone
pub fn spawn_hit_marker(
    commands: &mut Commands,
    time: &Time,
    is_kill: bool,
) {
    let color = if is_kill {
        Color::srgba(1.0, 0.2, 0.2, 1.0) // Red for kill
    } else {
        Color::srgba(1.0, 1.0, 1.0, 1.0) // White for hit
    };
    
    commands.spawn((
        HitMarker {
            spawn_time: time.elapsed_secs(),
            is_kill,
        },
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            position_type: PositionType::Absolute,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
    )).with_children(|parent| {
        // X shape for hit marker
        let line_length = if is_kill { 16.0 } else { 12.0 };
        let line_width = if is_kill { 3.0 } else { 2.0 };
        
        // Top-left to center
        parent.spawn((
            Node {
                width: Val::Px(line_length),
                height: Val::Px(line_width),
                position_type: PositionType::Absolute,
                top: Val::Px(-line_length / 2.0),
                left: Val::Px(-line_length / 2.0),
                ..default()
            },
            BackgroundColor(color),
            Transform::from_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
        ));
        
        // Top-right to center
        parent.spawn((
            Node {
                width: Val::Px(line_length),
                height: Val::Px(line_width),
                position_type: PositionType::Absolute,
                top: Val::Px(-line_length / 2.0),
                right: Val::Px(-line_length / 2.0),
                ..default()
            },
            BackgroundColor(color),
            Transform::from_rotation(Quat::from_rotation_z(-std::f32::consts::FRAC_PI_4)),
        ));
        
        // Bottom-left to center
        parent.spawn((
            Node {
                width: Val::Px(line_length),
                height: Val::Px(line_width),
                position_type: PositionType::Absolute,
                bottom: Val::Px(-line_length / 2.0),
                left: Val::Px(-line_length / 2.0),
                ..default()
            },
            BackgroundColor(color),
            Transform::from_rotation(Quat::from_rotation_z(-std::f32::consts::FRAC_PI_4)),
        ));
        
        // Bottom-right to center
        parent.spawn((
            Node {
                width: Val::Px(line_length),
                height: Val::Px(line_width),
                position_type: PositionType::Absolute,
                bottom: Val::Px(-line_length / 2.0),
                right: Val::Px(-line_length / 2.0),
                ..default()
            },
            BackgroundColor(color),
            Transform::from_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_4)),
        ));
    });
}

/// Update and cleanup hit markers
pub fn update_hit_markers(
    mut commands: Commands,
    mut hit_markers: Query<(Entity, &HitMarker, &mut BackgroundColor)>,
    time: Res<Time>,
) {
    let current_time = time.elapsed_secs();
    let hit_duration = 0.15;
    let kill_duration = 0.3;
    
    for (entity, marker, mut _bg) in hit_markers.iter_mut() {
        let duration = if marker.is_kill { kill_duration } else { hit_duration };
        let elapsed = current_time - marker.spawn_time;
        
        if elapsed > duration {
            commands.entity(entity).despawn();
        }
    }
}

/// Despawn crosshair and hit markers when leaving gameplay
pub fn despawn_crosshair(
    mut commands: Commands,
    crosshairs: Query<Entity, With<Crosshair>>,
    hit_markers: Query<Entity, With<HitMarker>>,
) {
    for entity in crosshairs.iter() {
        commands.entity(entity).despawn();
    }
    for entity in hit_markers.iter() {
        commands.entity(entity).despawn();
    }
}

/// Pickable marker to ignore mouse events
#[derive(Component, Clone, Copy)]
pub struct Pickable;

impl Pickable {
    pub const IGNORE: Self = Self;
}
