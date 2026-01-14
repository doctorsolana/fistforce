//! Pause menu UI (in-game escape menu)
//!
//! Updated for Bevy 0.17 / Lightyear 0.25
//! Now includes graphics settings panel for troubleshooting flickering/performance.
//! Menu smoothly slides when opening/closing the graphics panel.

use bevy::prelude::*;
use bevy::app::AppExit;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use lightyear::prelude::client::*;

use crate::states::GameState;
use crate::systems::GraphicsSettings;
use crate::GameClient;
use super::styles::*;

pub struct PauseMenuPlugin;

impl Plugin for PauseMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PauseMenuState>();
        app.add_systems(OnEnter(GameState::Paused), spawn_pause_menu);
        app.add_systems(OnExit(GameState::Paused), (despawn_pause_menu, reset_menu_state));
        app.add_systems(
            Update,
            (
                button_interactions,
                handle_pause_actions,
                handle_graphics_toggles,
                animate_menu_transition,
            )
                .run_if(in_state(GameState::Paused)),
        );
        app.add_systems(Update, handle_escape_key.run_if(in_state(GameState::Playing)));
        app.add_systems(Update, handle_resume_key.run_if(in_state(GameState::Paused)));
        
        // Release cursor when entering pause
        app.add_systems(OnEnter(GameState::Paused), release_cursor);
    }
}

/// Tracks the pause menu animation state
#[derive(Resource, Default)]
struct PauseMenuState {
    graphics_open: bool,
    /// Animation progress: 0.0 = closed (centered), 1.0 = open (shifted left)
    transition: f32,
}

/// Marker for the pause menu root
#[derive(Component)]
struct PauseMenuRoot;

/// Marker for the content container that gets shifted
#[derive(Component)]
struct MenuContentContainer;

/// Marker for the main menu column (buttons)
#[derive(Component)]
struct MainMenuColumn;

/// Marker for the graphics settings panel
#[derive(Component)]
struct GraphicsSettingsPanel;

/// Pause menu button actions
#[derive(Component, Clone, Copy)]
enum PauseButton {
    Resume,
    Graphics,
    Disconnect,
    Exit,
}

/// Graphics toggle buttons
#[derive(Component, Clone, Copy, Debug)]
enum GraphicsToggle {
    Bloom,
    Shadows,
    Atmosphere,
    Moonlight,
}

/// Marker for toggle button text (so we can update it)
#[derive(Component)]
struct ToggleText(GraphicsToggle);

fn spawn_pause_menu(mut commands: Commands, settings: Res<GraphicsSettings>) {
    // Full-screen darkened overlay
    commands
        .spawn((
            PauseMenuRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        ))
        .with_children(|parent| {
            // Content container - this is what we shift left/right
            // Using left margin to offset from center
            parent
                .spawn((
                    MenuContentContainer,
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(50.0),
                        // Start with 0 offset (perfectly centered)
                        margin: UiRect::left(Val::Px(0.0)),
                        ..default()
                    },
                ))
                .with_children(|container| {
                    // Main menu column
                    container
                        .spawn((
                            MainMenuColumn,
                            Node {
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                        ))
                        .with_children(|col| {
                            // Pause title
                            col.spawn((
                                Text::new("PAUSED"),
                                title_text_style(),
                                TextColor(TEXT_COLOR),
                                Node {
                                    margin: UiRect::bottom(Val::Px(40.0)),
                                    ..default()
                                },
                            ));

                            // Resume button
                            spawn_button(col, "RESUME", PauseButton::Resume);

                            // Graphics button
                            spawn_button(col, "GRAPHICS", PauseButton::Graphics);

                            // Disconnect button
                            spawn_button(col, "DISCONNECT", PauseButton::Disconnect);

                            // Exit button
                            spawn_button(col, "EXIT GAME", PauseButton::Exit);

                            // Hint
                            col.spawn((
                                Text::new("Press ESC to resume"),
                                TextFont {
                                    font_size: 14.0,
                                    ..default()
                                },
                                TextColor(TEXT_MUTED),
                                Node {
                                    margin: UiRect::top(Val::Px(30.0)),
                                    ..default()
                                },
                            ));
                        });

                    // Graphics settings panel (hidden by default, appears to the right)
                    spawn_graphics_panel(container, &settings);
                });
        });
}

fn spawn_button(parent: &mut ChildSpawnerCommands<'_>, text: &str, action: PauseButton) {
    parent
        .spawn((
            Button,
            action,
            button_style(),
            BackgroundColor(BUTTON_NORMAL),
            BorderRadius::all(Val::Px(4.0)),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(text),
                button_text_style(),
                TextColor(TEXT_COLOR),
            ));
        });
}

fn spawn_graphics_panel(parent: &mut ChildSpawnerCommands<'_>, settings: &GraphicsSettings) {
    parent
        .spawn((
            GraphicsSettingsPanel,
            Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::FlexStart,
                padding: UiRect::all(Val::Px(24.0)),
                min_width: Val::Px(0.0),
                // Start hidden so it doesn't affect layout
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.055, 0.05, 0.95)),
            BorderRadius::all(Val::Px(12.0)),
        ))
        .with_children(|panel| {
            // Panel title
            panel.spawn((
                Text::new("GRAPHICS"),
                TextFont {
                    font_size: 26.0,
                    ..default()
                },
                TextColor(ACCENT_COLOR),
                Node {
                    margin: UiRect::bottom(Val::Px(8.0)),
                    ..default()
                },
            ));

            // Help text
            panel.spawn((
                Text::new("Toggle to fix flickering"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(TEXT_MUTED),
                Node {
                    margin: UiRect::bottom(Val::Px(20.0)),
                    ..default()
                },
            ));

            // Toggle buttons
            spawn_toggle(panel, "Bloom", GraphicsToggle::Bloom, settings.bloom_enabled);
            spawn_toggle(panel, "Shadows", GraphicsToggle::Shadows, settings.shadows_enabled);
            spawn_toggle(panel, "Atmosphere", GraphicsToggle::Atmosphere, settings.atmosphere_enabled);
            spawn_toggle(panel, "Moonlight", GraphicsToggle::Moonlight, settings.moonlight_enabled);
        });
}

fn spawn_toggle(parent: &mut ChildSpawnerCommands<'_>, label: &str, toggle: GraphicsToggle, enabled: bool) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            width: Val::Percent(100.0),
            margin: UiRect::bottom(Val::Px(12.0)),
            ..default()
        })
        .with_children(|row| {
            // Label
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: 18.0,
                    ..default()
                },
                TextColor(TEXT_COLOR),
                Node {
                    margin: UiRect::right(Val::Px(40.0)),
                    ..default()
                },
            ));

            // Toggle button
            let (text, color) = if enabled {
                ("ON", Color::srgb(0.2, 0.55, 0.3))
            } else {
                ("OFF", Color::srgb(0.55, 0.2, 0.2))
            };

            row.spawn((
                Button,
                toggle,
                Node {
                    width: Val::Px(60.0),
                    height: Val::Px(30.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(color),
                BorderRadius::all(Val::Px(6.0)),
            ))
            .with_children(|btn| {
                btn.spawn((
                    ToggleText(toggle),
                    Text::new(text),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(TEXT_COLOR),
                ));
            });
        });
}

fn despawn_pause_menu(mut commands: Commands, query: Query<Entity, With<PauseMenuRoot>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

fn reset_menu_state(mut state: ResMut<PauseMenuState>) {
    state.graphics_open = false;
    state.transition = 0.0;
}

/// Smoothly animate the menu transition when opening/closing graphics panel
fn animate_menu_transition(
    time: Res<Time>,
    mut state: ResMut<PauseMenuState>,
    mut container_query: Query<&mut Node, (With<MenuContentContainer>, Without<GraphicsSettingsPanel>)>,
    mut panel_query: Query<&mut Node, (With<GraphicsSettingsPanel>, Without<MenuContentContainer>)>,
) {
    let target = if state.graphics_open { 1.0 } else { 0.0 };
    let speed = 10.0; // Animation speed
    
    // Smoothly interpolate toward target
    let diff = target - state.transition;
    if diff.abs() > 0.001 {
        state.transition += diff * speed * time.delta_secs();
        state.transition = state.transition.clamp(0.0, 1.0);
    } else {
        state.transition = target;
    }
    
    // Shift the entire content container left when graphics opens
    // Negative margin moves it left, making room for the graphics panel on the right
    // Small shift: -80px when fully open
    let offset = -80.0 * state.transition;
    
    for mut node in container_query.iter_mut() {
        node.margin.left = Val::Px(offset);
    }
    
    // Show/hide the graphics panel and animate its appearance
    for mut node in panel_query.iter_mut() {
        if state.transition > 0.01 {
            node.display = Display::Flex;
            // Expand width as it appears
            node.min_width = Val::Px(260.0 * state.transition);
        } else {
            node.display = Display::None;
            node.min_width = Val::Px(0.0);
        }
    }
}

fn button_interactions(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor, Option<&GraphicsToggle>),
        (Changed<Interaction>, With<Button>),
    >,
    settings: Res<GraphicsSettings>,
) {
    for (interaction, mut bg_color, toggle_opt) in buttons.iter_mut() {
        // For toggle buttons, use green/red based on state
        if let Some(toggle) = toggle_opt {
            let enabled = match toggle {
                GraphicsToggle::Bloom => settings.bloom_enabled,
                GraphicsToggle::Shadows => settings.shadows_enabled,
                GraphicsToggle::Atmosphere => settings.atmosphere_enabled,
                GraphicsToggle::Moonlight => settings.moonlight_enabled,
            };
            
            let base_color = if enabled {
                Color::srgb(0.2, 0.55, 0.3)
            } else {
                Color::srgb(0.55, 0.2, 0.2)
            };
            
            *bg_color = match interaction {
                Interaction::Pressed => BackgroundColor(base_color.lighter(0.15)),
                Interaction::Hovered => BackgroundColor(base_color.lighter(0.08)),
                Interaction::None => BackgroundColor(base_color),
            };
        } else {
            // Regular menu buttons
            *bg_color = match interaction {
                Interaction::Pressed => BackgroundColor(BUTTON_PRESSED),
                Interaction::Hovered => BackgroundColor(BUTTON_HOVERED),
                Interaction::None => BackgroundColor(BUTTON_NORMAL),
            };
        }
    }
}

fn handle_pause_actions(
    buttons: Query<(&Interaction, &PauseButton), Changed<Interaction>>,
    mut next_state: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
    client_query: Query<Entity, With<GameClient>>,
    mut menu_state: ResMut<PauseMenuState>,
) {
    for (interaction, action) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            match action {
                PauseButton::Resume => {
                    next_state.set(GameState::Playing);
                }
                PauseButton::Graphics => {
                    // Toggle the graphics panel (animation handled by animate_menu_transition)
                    menu_state.graphics_open = !menu_state.graphics_open;
                }
                PauseButton::Disconnect => {
                    info!("Disconnecting from server...");
                    // In Lightyear 0.25, trigger Disconnect on the client entity
                    if let Some(client_entity) = client_query.iter().next() {
                        commands.trigger(Disconnect { entity: client_entity });
                    }
                    next_state.set(GameState::MainMenu);
                }
                PauseButton::Exit => {
                    info!("Exiting game...");
                    exit.write(AppExit::Success);
                }
            }
        }
    }
}

fn handle_graphics_toggles(
    buttons: Query<(&Interaction, &GraphicsToggle), Changed<Interaction>>,
    mut settings: ResMut<GraphicsSettings>,
    mut toggle_texts: Query<(&ToggleText, &mut Text)>,
    mut toggle_buttons: Query<(&GraphicsToggle, &mut BackgroundColor), With<Button>>,
) {
    for (interaction, toggle) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            // Toggle the setting
            let new_value = match toggle {
                GraphicsToggle::Bloom => {
                    settings.bloom_enabled = !settings.bloom_enabled;
                    settings.bloom_enabled
                }
                GraphicsToggle::Shadows => {
                    settings.shadows_enabled = !settings.shadows_enabled;
                    settings.shadows_enabled
                }
                GraphicsToggle::Atmosphere => {
                    settings.atmosphere_enabled = !settings.atmosphere_enabled;
                    settings.atmosphere_enabled
                }
                GraphicsToggle::Moonlight => {
                    settings.moonlight_enabled = !settings.moonlight_enabled;
                    settings.moonlight_enabled
                }
            };

            info!("Graphics toggle {:?} = {}", toggle, new_value);

            // Update the text
            for (toggle_text, mut text) in toggle_texts.iter_mut() {
                if std::mem::discriminant(&toggle_text.0) == std::mem::discriminant(toggle) {
                    text.0 = if new_value { "ON".to_string() } else { "OFF".to_string() };
                }
            }

            // Update the button color
            let new_color = if new_value {
                Color::srgb(0.2, 0.55, 0.3)
            } else {
                Color::srgb(0.55, 0.2, 0.2)
            };
            
            for (btn_toggle, mut bg_color) in toggle_buttons.iter_mut() {
                if std::mem::discriminant(btn_toggle) == std::mem::discriminant(toggle) {
                    *bg_color = BackgroundColor(new_color);
                }
            }
        }
    }
}

fn handle_escape_key(keyboard: Res<ButtonInput<KeyCode>>, mut next_state: ResMut<NextState<GameState>>) {
    if keyboard.just_pressed(KeyCode::Escape) {
        next_state.set(GameState::Paused);
    }
}

fn handle_resume_key(keyboard: Res<ButtonInput<KeyCode>>, mut next_state: ResMut<NextState<GameState>>) {
    if keyboard.just_pressed(KeyCode::Escape) {
        next_state.set(GameState::Playing);
    }
}

fn release_cursor(
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
) {
    if let Ok(window_entity) = windows.single() {
        if let Ok(mut cursor) = cursor_opts.get_mut(window_entity) {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    }
}
