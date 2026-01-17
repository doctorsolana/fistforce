//! Game Client - Renders the world and handles player input
//!
//! Updated for Lightyear 0.25 / Bevy 0.17

mod audio;
mod build_mode;
mod camera;
mod chest;
mod crosshair;
mod dialogue;
mod input;
mod pickup;
mod props;
mod states;
mod structures;
mod systems;
mod terrain;
mod ui;
mod weapons;
mod weapon_view;

use bevy::prelude::*;
use bevy::audio::{AudioPlugin, SpatialScale};
use bevy::asset::AssetPlugin;
use bevy::diagnostic::{EntityCountDiagnosticsPlugin, FrameTimeDiagnosticsPlugin};
use bevy::render::diagnostic::RenderDiagnosticsPlugin;
use bevy::render::settings::{RenderCreation, WgpuSettings, WgpuFeatures};
use bevy::render::RenderPlugin;
use bevy::window::WindowResolution;
use lightyear::prelude::client::ClientPlugins;
use shared::{protocol::*, weapons::WeaponDebugMode, ProtocolPlugin, SERVER_ADDR, SERVER_PORT};
use states::GameState;

/// Marker component for our client entity
#[derive(Component)]
pub struct GameClient;

/// Get the asset path - for bundled macOS apps, use path relative to executable
fn get_asset_path() -> String {
    // Try to find assets relative to executable (for .app bundles)
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let bundled_assets = exe_dir.join("assets");
            if bundled_assets.exists() {
                info!("Using bundled assets at: {:?}", bundled_assets);
                return bundled_assets.to_string_lossy().to_string();
            }
        }
    }
    // Fall back to default "assets" folder (for development)
    "assets".to_string()
}

fn main() {
    let asset_path = get_asset_path();
    
    let mut app = App::new();

    // Full Bevy with rendering - configure asset path for bundled apps
    // Performance: Disable MSAA (expensive), enable GPU-driven rendering
    app.add_plugins(DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "Sandbox Game".to_string(),
                resolution: WindowResolution::new(1280, 720),
                ..default()
            }),
            ..default()
        })
        .set(AssetPlugin {
            file_path: asset_path,
            ..default()
        })
        .set(RenderPlugin {
            render_creation: RenderCreation::Automatic(WgpuSettings {
                // Enable GPU-driven rendering for better batching
                features: WgpuFeatures::INDIRECT_FIRST_INSTANCE,
                ..default()
            }),
            ..default()
        })
        // Spatial audio in rodio uses strong inverse-square falloff. Our world units are ~meters
        // (player height ~1.8), so we scale distances down to make spatial audio audible.
        .set(AudioPlugin {
            default_spatial_scale: SpatialScale::new(0.2),
            ..default()
        })
    );
    
    // FPS diagnostics for debug overlay
    app.add_plugins(FrameTimeDiagnosticsPlugin::default());
    // Extra diagnostics for the debug overlay (entity count + render pass timings)
    app.add_plugins(EntityCountDiagnosticsPlugin::default());
    app.add_plugins(RenderDiagnosticsPlugin::default());

    // Game state machine
    app.init_state::<GameState>();

    // Lightyear client plugins (tick_duration = 60Hz)
    // In Lightyear 0.25, we just add the plugins with tick duration
    app.add_plugins(ClientPlugins {
        tick_duration: tick_duration(),
    });
    app.add_plugins(ProtocolPlugin);

    // Terrain generation and rendering
    app.add_plugins(terrain::TerrainPlugin);
    
    // Environmental props (rocks, trees, etc.)
    app.add_plugins(props::PropsPlugin);
    
    // Desert settlement structures
    app.add_plugins(structures::StructuresPlugin);

    // UI plugins
    app.add_plugins(ui::MainMenuPlugin);
    app.add_plugins(ui::PauseMenuPlugin);
    app.add_plugins(ui::InventoryPlugin);
    app.add_plugins(ui::NameEntryPlugin);
    
    // Pickup plugin (item pickups with E key)
    app.add_plugins(pickup::PickupPlugin);
    
    // Chest plugin (storage containers)
    app.add_plugins(chest::ChestPlugin);
    
    // Build mode plugin
    app.add_plugins(build_mode::BuildModePlugin);
    
    // Audio plugin
    app.add_plugins(audio::GameAudioPlugin);

    // NPC Dialogue plugin
    app.add_plugins(dialogue::DialoguePlugin);

    // Setup systems (run once at startup - rendering only)
    app.add_systems(Startup, (
        systems::setup_rendering,
        systems::setup_particle_assets,
        weapons::setup_weapon_visual_assets,
        weapons::setup_weapon_audio_assets,
        systems::setup_player_character_assets,
        systems::setup_npc_assets,
    ));
    
    // Weapon debug mode resource
    app.init_resource::<WeaponDebugMode>();
    app.init_resource::<weapons::PerfOverlayEnabled>();
    app.init_resource::<weapons::ShootingState>();
    app.init_resource::<weapons::ReloadState>();
    app.init_resource::<weapons::DebugBulletTrails>();
    app.init_resource::<weapon_view::CurrentWeaponView>();
    app.init_resource::<weapon_view::CurrentThirdPersonWeapon>();
    
    // Graphics settings (toggleable from pause menu)
    app.init_resource::<systems::GraphicsSettings>();

    // Input settings (controls, sensitivity - adjustable from pause menu)
    app.init_resource::<systems::InputSettings>();

    // Ensure we clean up visuals when entering menu
    app.add_systems(OnEnter(GameState::MainMenu), systems::enter_main_menu);

    // Connection systems
    app.add_systems(OnEnter(GameState::Connecting), systems::start_connection);
    app.add_systems(
        Update,
        systems::check_connection.run_if(in_state(GameState::Connecting)),
    );

    // Spawn world visuals, HUD, crosshair, and death screen when entering gameplay
    app.add_systems(OnEnter(GameState::Playing), (
        systems::spawn_world,
        crosshair::spawn_crosshair,
        crosshair::spawn_death_screen,
        weapon_view::spawn_weapon_hud,
        weapons::spawn_debug_overlay,
    ));
    
    // Cleanup HUD, crosshair, and death screen when leaving gameplay
    app.add_systems(OnExit(GameState::Playing), (
        crosshair::despawn_crosshair,
        crosshair::despawn_death_screen,
        weapon_view::despawn_weapon_hud,
        weapon_view::despawn_third_person_weapon,
        weapon_view::despawn_remote_third_person_weapons,
        weapons::despawn_debug_overlay,
    ));

    // Send input to server at fixed tick rate (60 Hz)
    app.add_systems(
        FixedUpdate,
        input::send_input_to_server
            .run_if(in_state(GameState::Playing).or(in_state(GameState::Paused))),
    );

    // Replication-driven spawn/setup must NOT be gated solely to `Playing`.
    // Initial snapshots can arrive while we're still in `Connecting` (especially over WAN),
    // which would cause `Added<T>` handlers to miss and leave the client in a "ghost" state.
    app.add_systems(
        Update,
        (
            systems::handle_player_spawned,
            systems::handle_npc_spawned,
            systems::handle_vehicle_spawned,
            systems::ensure_local_player_tag,
        )
            .chain()
            .run_if(in_state(GameState::Connecting).or(in_state(GameState::Playing))),
    );

    // Gameplay systems (only when playing) - split into groups to avoid tuple limit
    // ORDER MATTERS (and we enforce it): vehicles -> players (attach to vehicles) -> camera.
    app.add_systems(
        Update,
        (
            input::handle_keyboard_input,
            input::update_vehicle_state,
            input::handle_mouse_input,
            input::update_death_state,
            systems::grab_cursor,
            // Hard-chain the render pose pipeline so we never read a stale vehicle transform.
            (
                systems::sync_vehicle_transforms,
                systems::sync_player_transforms,
                systems::sync_npc_transforms,
                camera::update_camera,
            )
                .chain(),
            camera::update_camera_fov,
            systems::spawn_sand_particles,
            systems::update_sand_particles,
            systems::update_day_night_cycle,
            systems::apply_graphics_settings,
        )
            .run_if(in_state(GameState::Playing)),
    );

    // Player character visuals/animation (KayKit Ranger)
    app.add_systems(
        Update,
        (
            systems::setup_ranger_rig,
            systems::update_ranger_animation,
            systems::update_local_player_visibility,
        )
            .run_if(in_state(GameState::Playing)),
    );

    // NPC visuals/animation + debug hitboxes
    app.add_systems(
        Update,
        (
            systems::setup_npc_rig,
            systems::update_npc_animation,
            systems::debug_draw_npc_hitboxes,
        )
            .run_if(in_state(GameState::Playing)),
    );
    
    // Weapon and UI systems (only when playing) - split into smaller groups
    app.add_systems(
        Update,
        (
            crosshair::update_crosshair_visibility,
            crosshair::update_crosshair_ads,
            crosshair::update_hit_markers,
            crosshair::update_death_screen,
        )
            .run_if(in_state(GameState::Playing)),
    );
    
    app.add_systems(
        Update,
        (
            weapons::handle_shoot_input,
            weapons::handle_reload_input,
            weapons::play_weapon_sounds,
            weapons::handle_bullet_spawned,
            weapons::recover_recoil,
        )
            .run_if(in_state(GameState::Playing)),
    );
    
    app.add_systems(
        Update,
        (
            weapons::update_bullet_visuals,
            weapons::update_local_tracers,
            weapons::handle_bullet_impacts,
        )
            .run_if(in_state(GameState::Playing)),
    );
    
    app.add_systems(
        Update,
        (
            weapons::update_impact_markers,
            weapons::update_blood_bursts,
            weapons::update_blood_droplets,
            weapons::update_blood_splash_rings,
            weapons::update_blood_ground_splats,
            weapons::handle_hit_confirms,
            weapons::toggle_perf_overlay,
            weapons::toggle_debug_mode,
            weapons::debug_draw_trajectories,
            weapons::update_debug_overlay,
        )
            .run_if(in_state(GameState::Playing)),
    );
    
    // Weapon view systems (3D models and HUD)
    app.add_systems(
        Update,
        (
            weapon_view::handle_weapon_switch,
            weapon_view::update_weapon_hud,
            weapon_view::update_first_person_weapon,
            weapon_view::update_third_person_weapon,
            weapon_view::update_remote_third_person_weapons,
            weapon_view::animate_weapon,
        )
            .run_if(in_state(GameState::Playing)),
    );

    // Input resource
    app.init_resource::<input::InputState>();
    app.init_resource::<systems::LastCameraMode>();

    // Generate a unique client ID for logging
    let client_id = rand::random::<u64>();
    info!(
        "Starting client, server at {}:{}, client_id: {}",
        SERVER_ADDR, SERVER_PORT, client_id
    );
    app.run();
}
