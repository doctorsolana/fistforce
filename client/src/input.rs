//! Player input handling
//!
//! Updated for Lightyear 0.25 / Bevy 0.17

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use lightyear::prelude::*;
use shared::{InputChannel, PlayerInput, VehicleInput, VehicleDriver, LocalPlayer, Player, MOUSE_SENSITIVITY};
use std::f32::consts::FRAC_PI_2;

use crate::states::GameState;

/// Camera view mode
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum CameraMode {
    #[default]
    FirstPerson,
    ThirdPerson,
}

/// Client-side input state
#[derive(Resource)]
pub struct InputState {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    /// Jump request (spacebar)
    pub jump: bool,
    /// Mouse-controlled yaw (used when on foot)
    pub yaw: f32,
    /// Mouse-controlled pitch
    pub pitch: f32,
    pub interact: bool,
    pub interact_just_pressed: bool,
    /// Hold Shift for air tricks (pitch/roll control while airborne)
    pub shift: bool,
    
    /// Camera mode (toggle with P)
    pub camera_mode: CameraMode,
    
    // Vehicle camera state
    /// True when we are currently driving a vehicle (used for camera + mouse look behavior)
    pub in_vehicle: bool,
    /// When in vehicle: relative look offset from center (for looking around)
    pub vehicle_look_yaw: f32,
    pub vehicle_look_pitch: f32,
    
    /// Right-click = Aim Down Sights
    pub aiming: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            forward: false,
            backward: false,
            left: false,
            right: false,
            jump: false,
            yaw: 0.0,
            pitch: 0.0,
            interact: false,
            interact_just_pressed: false,
            shift: false,
            camera_mode: CameraMode::FirstPerson,
            in_vehicle: false,
            vehicle_look_yaw: 0.0,
            vehicle_look_pitch: 0.0,
            aiming: false,
        }
    }
}

/// Handle keyboard input for movement
pub fn handle_keyboard_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut input_state: ResMut<InputState>,
) {
    input_state.forward = keyboard.pressed(KeyCode::KeyW);
    input_state.backward = keyboard.pressed(KeyCode::KeyS);
    input_state.left = keyboard.pressed(KeyCode::KeyA);
    input_state.right = keyboard.pressed(KeyCode::KeyD);
    input_state.jump = keyboard.pressed(KeyCode::Space);
    input_state.shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    
    input_state.interact_just_pressed = keyboard.just_pressed(KeyCode::KeyE);
    input_state.interact = input_state.interact_just_pressed;
    
    // Toggle camera mode with P
    if keyboard.just_pressed(KeyCode::KeyP) {
        input_state.camera_mode = match input_state.camera_mode {
            CameraMode::FirstPerson => CameraMode::ThirdPerson,
            CameraMode::ThirdPerson => CameraMode::FirstPerson,
        };
        info!("Camera mode: {:?}", 
            if input_state.camera_mode == CameraMode::FirstPerson { "First Person" } else { "Third Person" });
    }
}

/// Handle mouse input for looking around
pub fn handle_mouse_input(
    mut mouse_motion: MessageReader<MouseMotion>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut input_state: ResMut<InputState>,
) {
    // Track ADS (right-click)
    input_state.aiming = mouse_button.pressed(MouseButton::Right) && !input_state.in_vehicle;
    
    let mut delta = Vec2::ZERO;
    for motion in mouse_motion.read() {
        delta += motion.delta;
    }

    if delta != Vec2::ZERO {
        // Reduce sensitivity when aiming for more precise control
        let sensitivity = if input_state.aiming {
            MOUSE_SENSITIVITY * 0.5
        } else {
            MOUSE_SENSITIVITY
        };
        
        if input_state.in_vehicle {
            // In vehicle: update relative look angles
            input_state.vehicle_look_yaw -= delta.x * sensitivity;
            input_state.vehicle_look_pitch -= delta.y * sensitivity;
            
            // Clamp: can look ~90° left/right, ~60° up/down
            input_state.vehicle_look_yaw = input_state.vehicle_look_yaw.clamp(-FRAC_PI_2, FRAC_PI_2);
            input_state.vehicle_look_pitch = input_state.vehicle_look_pitch.clamp(-0.5, 0.7);
        } else {
            // On foot: free look
            input_state.yaw -= delta.x * sensitivity;
            input_state.pitch -= delta.y * sensitivity;
            input_state.pitch = input_state.pitch.clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
        }
    }
}

/// Helper to convert PeerId to u64 for driver tracking
fn peer_id_to_u64(peer_id: PeerId) -> u64 {
    match peer_id {
        PeerId::Netcode(id) => id,
        PeerId::Steam(id) => id,
        PeerId::Local(id) => id,
        _ => 0, // Server or other types
    }
}

/// Update input state with whether we're driving a vehicle.
/// (The camera uses the *smoothed vehicle Transform* directly; this is just for mouse-look mode.)
pub fn update_vehicle_state(
    mut input_state: ResMut<InputState>,
    // In Lightyear 0.25, use LocalId to identify the local client peer id
    client_query: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    vehicles: Query<&VehicleDriver>,
) {
    // Get our peer ID from the connected client entity
    let Some(our_peer_id) = client_query.iter().next().map(|r| r.0) else {
        return;
    };

    let is_driving = vehicles
        .iter()
        .any(|driver| driver.driver_id == Some(peer_id_to_u64(our_peer_id)));

    // If we just exited, reset look offsets.
    if input_state.in_vehicle && !is_driving {
        input_state.vehicle_look_yaw = 0.0;
        input_state.vehicle_look_pitch = 0.0;
    }

    input_state.in_vehicle = is_driving;
}

/// Send input to server
pub fn send_input_to_server(
    input_state: Res<InputState>,
    game_state: Res<State<GameState>>,
    // In Lightyear 0.25, send messages via MessageSender component - typed on message type
    mut client_query: Query<(&LocalId, &mut MessageSender<PlayerInput>), (With<crate::GameClient>, With<Connected>)>,
    local_player: Query<&Player, With<LocalPlayer>>,
    vehicles: Query<&VehicleDriver>,
    time: Res<Time>,
    mut last_warn_time: Local<f32>,
) {
    // Get client entity with sender
    let Ok((local_id, mut sender)) = client_query.single_mut() else {
        // If this fires, input will *never* reach the server, so movement will be frozen.
        let now = time.elapsed_secs();
        if now - *last_warn_time > 1.0 {
            warn!("send_input_to_server: missing GameClient+Connected+LocalId+MessageSender<PlayerInput>; not sending inputs");
            *last_warn_time = now;
        }
        return;
    };
    
    let our_peer_id = local_id.0;
    let in_vehicle = vehicles.iter().any(|driver| {
        driver.driver_id == Some(peer_id_to_u64(our_peer_id))
    });
    
    let _has_local_player = local_player.iter().next().is_some();
    
    let mut input = PlayerInput {
        forward: input_state.forward,
        backward: input_state.backward,
        left: input_state.left,
        right: input_state.right,
        jump: input_state.jump,
        yaw: input_state.yaw,
        vehicle_input: None,
        interact: input_state.interact_just_pressed,
    };

    if game_state.get() == &GameState::Paused {
        input.forward = false;
        input.backward = false;
        input.left = false;
        input.right = false;
        input.jump = false;
        input.interact = false;
    }

    if in_vehicle && game_state.get() != &GameState::Paused {
        input.vehicle_input = Some(VehicleInput {
            throttle: if input_state.forward { 1.0 } else { 0.0 },
            brake: if input_state.backward { 1.0 } else { 0.0 },
            steer: if input_state.left { 1.0 } else if input_state.right { -1.0 } else { 0.0 },
            air_control: input_state.shift, // Hold Shift for air tricks
        });
        input.forward = false;
        input.backward = false;
        input.left = false;
        input.right = false;
        input.jump = false; // Can't jump while in vehicle
    }

    let _ = sender.send::<InputChannel>(input);
}
