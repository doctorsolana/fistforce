//! Rendering systems
//!
//! Atmosphere, day/night cycle, and camera setup.

use bevy::prelude::*;
use bevy::audio::SpatialListener;
use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{light_consts::lux, AtmosphereEnvironmentMapLight, DirectionalLightShadowMap};
use bevy::pbr::{Atmosphere, AtmosphereMode, AtmosphereSettings};
use bevy::post_process::bloom::Bloom;
use bevy::render::view::Msaa;

// =============================================================================
// COMPONENTS
// =============================================================================

/// Marker for the sun directional light (driven by day/night cycle)
#[derive(Component)]
pub struct SunLight;

/// Marker for the moon directional light (provides visibility at night)
#[derive(Component)]
pub struct MoonLight;

// =============================================================================
// ATMOSPHERE CONFIGURATION
// =============================================================================

/// Desert atmosphere preset - more dust, redder sunsets, brighter sand
/// Based on Earth but with increased Mie scattering for that UAE/Arabian desert look
const DESERT_ATMOSPHERE: Atmosphere = Atmosphere {
    // Same planet dimensions as Earth
    bottom_radius: 6_360_000.0,
    top_radius: 6_460_000.0,
    // Sandy desert surface is more reflective (warm tones)
    ground_albedo: Vec3::new(0.45, 0.38, 0.28),
    // Standard Rayleigh (blue sky) - slightly reduced for drier desert air
    rayleigh_density_exp_scale: 1.0 / 8_500.0,
    rayleigh_scattering: Vec3::new(5.0e-6, 12.0e-6, 28.0e-6),
    // INCREASED Mie scattering - desert dust! This is key for red sunsets
    mie_density_exp_scale: 1.0 / 800.0,  // Dust extends higher
    mie_scattering: 12.0e-6,             // 3x Earth's default - more dust particles
    mie_absorption: 1.5e-6,              // Slightly more absorption too
    mie_asymmetry: 0.75,                 // Slightly more isotropic scattering
    // Less ozone for desert environment
    ozone_layer_altitude: 25_000.0,
    ozone_layer_width: 30_000.0,
    ozone_absorption: Vec3::new(0.5e-6, 1.5e-6, 0.07e-6),
};

/// Atmosphere LUT settings tuned for *realtime gameplay*.
///
/// Bevy's defaults look great but can be expensive:
/// - sky_view_lut_size default: 400x200
/// - aerial_view_lut_size default: 32^3
///
/// Lowering these tends to recover a lot of FPS, especially on integrated GPUs.
fn desert_atmosphere_settings_perf() -> AtmosphereSettings {
    AtmosphereSettings {
        // Global LUTs (aggressively reduced for performance)
        transmittance_lut_size: UVec2::new(128, 64),
        transmittance_lut_samples: 20,
        multiscattering_lut_size: UVec2::new(16, 16),
        multiscattering_lut_dirs: 32,
        multiscattering_lut_samples: 10,

        // View-dependent LUTs (big wins - reduced further)
        sky_view_lut_size: UVec2::new(192, 96),
        sky_view_lut_samples: 8,
        aerial_view_lut_size: UVec3::new(16, 16, 16),
        aerial_view_lut_samples: 4,
        // Smaller max distance - don't need aerial perspective past 10km
        aerial_view_lut_max_distance: 1.0e4,

        // 1 unit = 1 meter in our world
        scene_units_to_m: 1.0,

        // Fallback cap used in some paths
        sky_max_samples: 8,

        rendering_method: AtmosphereMode::LookupTexture,
    }
}

// =============================================================================
// SETUP
// =============================================================================

/// One-time rendering setup.
pub fn setup_rendering(mut commands: Commands) {
    // Performance: directional light shadows are expensive, especially with multiple cascades.
    // Lowering the shadow map resolution is a big win while keeping shadows enabled.
    commands.insert_resource(DirectionalLightShadowMap { size: 1024 });

    // With Atmosphere enabled, the sky is rendered procedurally, so ClearColor is mostly a fallback.
    commands.insert_resource(ClearColor(Color::BLACK));

    commands.spawn((
        Camera3d::default(),
        // Performance: Disable MSAA (big win, we use bloom/tonemapping for quality)
        Msaa::Off,
        // Nice highlights rolloff for HDR outdoor scenes.
        Tonemapping::AcesFitted,
        // Physical light levels (RAW_SUNLIGHT) are bright: slightly higher exposure for harsh desert sun
        Exposure::SUNLIGHT,
        // Desert atmosphere - more dust for red/orange sunsets
        DESERT_ATMOSPHERE,
        // Atmosphere settings (perf-tuned LUT sizes/samples).
        desert_atmosphere_settings_perf(),
        // Enhanced bloom for that blazing desert sun
        // Performance: Use fewer bloom passes (composite_mode)
        Bloom {
            intensity: 0.2,
            low_frequency_boost: 0.5,
            low_frequency_boost_curvature: 0.8,
            high_pass_frequency: 0.8,
            ..Bloom::NATURAL
        },
        // Let the atmosphere drive ambient/IBL lighting for this view.
        // Performance: Tiny cubemap - we mostly use the sun anyway
        AtmosphereEnvironmentMapLight {
            intensity: 0.8,
            affects_lightmapped_mesh_diffuse: true,
            size: UVec2::new(64, 64), // Reduced from 128 for perf
        },
        Transform::from_xyz(0.0, 10.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
        // Explicit spatial + visibility components (camera is parent of the first-person weapon model).
        GlobalTransform::default(),
        Visibility::default(),
        InheritedVisibility::default(),
        // Audio listener for spatial audio (other players' sounds)
        // SpatialListener defines where the "ears" are relative to the entity
        SpatialListener::new(0.1), // ~10cm between ears
    ));

    info!("Client rendering initialized with desert atmosphere");
}

// =============================================================================
// DAY/NIGHT CYCLE
// =============================================================================

/// Update sun, moon, ambient light, and sky color based on replicated WorldTime.
/// Creates beautiful sunrise/sunset transitions with visible moonlit nights.
pub fn update_day_night_cycle(
    world_time_query: Query<&shared::WorldTime>,
    mut sun_query: Query<(&mut DirectionalLight, &mut Transform), (With<SunLight>, Without<Camera3d>, Without<MoonLight>)>,
    mut moon_query: Query<(&mut DirectionalLight, &mut Transform), (With<MoonLight>, Without<Camera3d>, Without<SunLight>)>,
    mut ambient: ResMut<AmbientLight>,
    time: Res<Time>,
    mut debug_timer: Local<f32>,
) {
    *debug_timer += time.delta_secs();
    let should_log = *debug_timer > 3.0;
    if should_log {
        *debug_timer = 0.0;
    }

    // Get replicated world time (spawned by server)
    let world_time = match world_time_query.iter().next() {
        Some(wt) => wt,
        None => {
            if should_log {
                info!("Day/Night: Waiting for WorldTime from server...");
            }
            return;
        }
    };

    // Normalized time: 0.0 = midnight, 0.25 = sunrise, 0.5 = noon, 0.75 = sunset
    let t = world_time.normalized_time();

    // =========================================================================
    // SUN POSITION - Rotates around the world (rises in east, sets in west)
    // =========================================================================
    // IMPORTANT: we want elevation = -1 at midnight, 0 at sunrise/sunset, +1 at noon.
    let phase = t * std::f32::consts::TAU;
    let elevation = -phase.cos(); // [-1, 1]

    // Sunrise at t=0.25 should come from +X (east) -> rays point toward -X.
    let azimuth = phase - std::f32::consts::PI;

    // Convert elevation factor to an angle. Clamp so the sun doesn't go *perfectly* overhead.
    let elev_angle = elevation.clamp(-1.0, 1.0) * std::f32::consts::FRAC_PI_2 * 0.9;
    let cos_e = elev_angle.cos();
    let sin_e = elev_angle.sin();

    // Direction the light rays travel (from sun -> world). y is negative when sun is above horizon.
    let sun_dir = Vec3::new(azimuth.sin() * cos_e, -sin_e, azimuth.cos() * cos_e).normalize_or_zero();

    // =========================================================================
    // SUN INTENSITY
    // =========================================================================
    // Atmosphere handles the sky tinting; keep the directional light physically plausible.
    // "Sun height" factor: 0 at horizon, 1 at noon
    let sun_height = elevation.max(0.0);
    
    // Day factor: 1 during day, 0 at night
    let day_factor = smoothstep(-0.05, 0.15, elevation);
    
    // Sun intensity: RAW_SUNLIGHT at noon, fades out smoothly toward night.
    let sun_illuminance = lux::RAW_SUNLIGHT * sun_height.powf(0.6) * day_factor;

    // Bevy directional light points along -Z (forward). Rotate -Z to match sun_dir.
    for (mut sun_light, mut sun_transform) in sun_query.iter_mut() {
        sun_transform.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, sun_dir);
        sun_light.color = Color::WHITE;
        sun_light.illuminance = sun_illuminance;
    }

    // =========================================================================
    // MOONLIGHT - Opposite arc of the sun, bright at night
    // =========================================================================
    // Moon is on the opposite side of the sky from the sun
    let moon_dir = Vec3::new(-azimuth.sin() * cos_e, sin_e.abs().max(0.2), -azimuth.cos() * cos_e).normalize_or_zero();
    
    // Night factor: inverse of day factor (1 at night, 0 during day)
    let night_factor = 1.0 - day_factor;
    
    // Moon intensity: bright at night, fades during day
    // Boosted significantly for gameplay visibility (real moonlight is ~0.3 lux, we use ~800)
    let moon_illuminance = 800.0 * night_factor;
    
    for (mut moon_light, mut moon_transform) in moon_query.iter_mut() {
        moon_transform.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, moon_dir);
        // Cool silvery-blue moonlight
        moon_light.color = Color::srgb(0.7, 0.8, 1.0);
        moon_light.illuminance = moon_illuminance;
    }

    // =========================================================================
    // AMBIENT LIGHT - Desert environment: warm during day, cool blue at night
    // =========================================================================
    // With `AtmosphereEnvironmentMapLight`, most ambient should come from the atmosphere-driven IBL.
    // Boost brightness significantly for harsh desert daylight.
    let day_ambient_color = Color::srgb(0.9, 0.85, 0.75);  // Warm sandy tones during day
    let night_ambient_color = Color::srgb(0.15, 0.18, 0.25);  // Cool blue-ish night (brighter for visibility)
    ambient.color = lerp_color(night_ambient_color, day_ambient_color, day_factor);
    // Night ambient is now 12 lux (up from 1) so you can actually see
    // Day ambient peaks at 80 lux for that blazing desert sun feel
    ambient.brightness = 12.0 + 68.0 * day_factor;
}

// =============================================================================
// HELPERS
// =============================================================================

/// Helper to linearly interpolate between two colors
fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let a_rgba = a.to_srgba();
    let b_rgba = b.to_srgba();
    Color::srgba(
        a_rgba.red + (b_rgba.red - a_rgba.red) * t,
        a_rgba.green + (b_rgba.green - a_rgba.green) * t,
        a_rgba.blue + (b_rgba.blue - a_rgba.blue) * t,
        a_rgba.alpha + (b_rgba.alpha - a_rgba.alpha) * t,
    )
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
