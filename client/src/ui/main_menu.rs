//! Main menu UI
//!
//! Updated for Bevy 0.17 with server preset dropdown

use bevy::prelude::*;
use bevy::app::AppExit;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use serde::Deserialize;

use crate::states::GameState;
use super::styles::*;
use shared::SERVER_PORT;

pub struct MainMenuPlugin;

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        // Load server presets synchronously during plugin build (before any systems run)
        let (presets, server_address) = load_server_presets_sync();
        app.insert_resource(presets);
        app.insert_resource(server_address);
        app.init_resource::<DropdownState>();
        
        app.add_systems(OnEnter(GameState::MainMenu), spawn_main_menu);
        app.add_systems(OnExit(GameState::MainMenu), despawn_main_menu);
        app.add_systems(
            Update,
            (
                button_interactions,
                handle_menu_actions,
                animate_logo,
                handle_ip_input_focus,
                handle_ip_keyboard_input,
                update_ip_display,
                handle_dropdown_toggle,
                handle_dropdown_selection,
                update_dropdown_display,
            ).run_if(in_state(GameState::MainMenu)),
        );
    }
}

// =============================================================================
// CONFIG TYPES
// =============================================================================

/// A single server entry from the config file
#[derive(Debug, Clone, Deserialize)]
pub struct ServerEntry {
    pub name: String,
    pub ip: String,
}

/// The servers.ron config file structure
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub servers: Vec<ServerEntry>,
    pub default_index: usize,
}

/// Resource holding loaded server presets
#[derive(Resource, Default)]
pub struct ServerPresets {
    pub entries: Vec<ServerEntry>,
    pub selected_index: Option<usize>,
}

/// Resource holding the server address to connect to
#[derive(Resource)]
pub struct ServerAddress {
    pub ip: String,
    pub port: u16,
}

impl Default for ServerAddress {
    fn default() -> Self {
        Self {
            ip: "127.0.0.1".to_string(),
            port: SERVER_PORT,
        }
    }
}

/// Dropdown open/closed state
#[derive(Resource, Default)]
pub struct DropdownState {
    pub expanded: bool,
}

// =============================================================================
// COMPONENTS
// =============================================================================

/// Marker for the main menu root
#[derive(Component)]
struct MainMenuRoot;

/// Marker for the logo (for animation)
#[derive(Component)]
struct LogoImage {
    base_scale: f32,
    time: f32,
}

/// Marker for the IP input field
#[derive(Component)]
struct IpInputField {
    focused: bool,
}

/// Marker for the IP text display
#[derive(Component)]
struct IpTextDisplay;

/// Button action types
#[derive(Component, Clone, Copy)]
enum MenuButton {
    Connect,
    Exit,
}

/// Dropdown toggle button
#[derive(Component)]
struct DropdownToggle;

/// Dropdown text display (shows selected preset name)
#[derive(Component)]
struct DropdownText;

/// Container for dropdown options (shown when expanded)
#[derive(Component)]
struct DropdownOptions;

/// A single dropdown option
#[derive(Component)]
struct DropdownOption {
    index: usize,
}

// =============================================================================
// STARTUP: LOAD CONFIG
// =============================================================================

/// Load server presets synchronously (called during plugin build)
fn load_server_presets_sync() -> (ServerPresets, ServerAddress) {
    // In dev, assets live under `client/assets/`.
    // In packaged builds (e.g. macOS .app), assets are bundled as `assets/` next to the executable.
    let candidates = ["assets/servers.ron", "client/assets/servers.ron"];

    let found: Option<(&str, String)> = candidates
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok().map(|c| (*p, c)));

    let config: Option<ServerConfig> = found
        .as_ref()
        .and_then(|(_p, c)| ron::from_str(c).ok());
    
    if let Some(config) = config {
        info!(
            "Loaded {} server presets from {}",
            config.servers.len(),
            found.as_ref().map(|(p, _)| *p).unwrap_or("<unknown>")
        );
        
        // Set default server address from config
        let ip = config.servers.get(config.default_index)
            .map(|e| e.ip.clone())
            .unwrap_or_else(|| "127.0.0.1".to_string());
        
        (
            ServerPresets {
                entries: config.servers,
                selected_index: Some(config.default_index),
            },
            ServerAddress {
                ip,
                port: SERVER_PORT,
            },
        )
    } else {
        warn!(
            "Could not load servers.ron (tried {:?}), using defaults",
            candidates
        );
        (ServerPresets::default(), ServerAddress::default())
    }
}

// =============================================================================
// MENU SPAWN
// =============================================================================

fn spawn_main_menu(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    server_address: Res<ServerAddress>,
    presets: Res<ServerPresets>,
    mut dropdown_state: ResMut<DropdownState>,
) {
    // Reset dropdown state when entering menu
    dropdown_state.expanded = false;
    
    // Load the logo
    let logo_handle: Handle<Image> = asset_server.load("ui/fistforce.png");

    commands
        .spawn((
            MainMenuRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(MENU_BACKGROUND),
        ))
        .with_children(|parent| {
            // Vignette overlay (dark edges for depth)
            parent.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.3)),
            ));

            // Logo image container
            parent.spawn((
                LogoImage {
                    base_scale: 1.0,
                    time: 0.0,
                },
                Node {
                    width: Val::Px(500.0),
                    height: Val::Px(281.25), // 16:9 aspect ratio (500 / 16 * 9)
                    margin: UiRect::bottom(Val::Px(40.0)),
                    ..default()
                },
                ImageNode::new(logo_handle),
            ));

            // Server IP input section
            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    margin: UiRect::bottom(Val::Px(20.0)),
                    ..default()
                })
                .with_children(|ip_section| {
                    // Label
                    ip_section.spawn((
                        Text::new("SERVER IP"),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(TEXT_MUTED),
                        Node {
                            margin: UiRect::bottom(Val::Px(8.0)),
                            ..default()
                        },
                    ));

                    // Input field container (clickable)
                    ip_section
                        .spawn((
                            IpInputField {
                                focused: false,
                            },
                            Button,
                            Node {
                                width: Val::Px(280.0),
                                height: Val::Px(45.0),
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(Val::Px(2.0)),
                                padding: UiRect::horizontal(Val::Px(12.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.08, 0.07, 0.06)),
                            BorderColor::from(BUTTON_BORDER),
                            BorderRadius::all(Val::Px(4.0)),
                        ))
                        .with_children(|input_box| {
                            // IP text
                            input_box.spawn((
                                IpTextDisplay,
                                Text::new(format!("{}:{}", server_address.ip, server_address.port)),
                                TextFont {
                                    font_size: 20.0,
                                    ..default()
                                },
                                TextColor(TEXT_COLOR),
                            ));
                        });
                    
                    // Server preset dropdown (only if we have presets)
                    if !presets.entries.is_empty() {
                        spawn_dropdown(ip_section, &presets);
                    }
                    
                    // Helper text
                    ip_section.spawn((
                        Text::new("Click to edit • Select preset below"),
                        TextFont {
                            font_size: 12.0,
                            ..default()
                        },
                        TextColor(TEXT_MUTED),
                        Node {
                            margin: UiRect::top(Val::Px(6.0)),
                            ..default()
                        },
                    ));
                });

            // Button container
            parent
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    padding: UiRect::all(Val::Px(10.0)),
                    ..default()
                })
                .with_children(|btn_container| {
                    // Connect button
                    spawn_button(btn_container, "CONNECT", MenuButton::Connect);

                    // Exit button
                    spawn_button(btn_container, "EXIT", MenuButton::Exit);
                });

            // Version info at bottom
            parent.spawn((
                Text::new("v0.1.0 | Bevy + Lightyear"),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(TEXT_MUTED),
                Node {
                    position_type: PositionType::Absolute,
                    bottom: Val::Px(20.0),
                    ..default()
                },
            ));
        });
}

fn spawn_dropdown(parent: &mut ChildSpawnerCommands<'_>, presets: &ServerPresets) {
    let selected_name = presets.selected_index
        .and_then(|i| presets.entries.get(i))
        .map(|e| e.name.as_str())
        .unwrap_or("Custom");
    
    // Dropdown container (relative positioning for absolute children)
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            margin: UiRect::top(Val::Px(8.0)),
            ..default()
        })
        .with_children(|dropdown_container| {
            // Toggle button (always visible)
            dropdown_container
                .spawn((
                    DropdownToggle,
                    Button,
                    Node {
                        width: Val::Px(280.0),
                        height: Val::Px(38.0),
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        border: UiRect::all(Val::Px(1.0)),
                        padding: UiRect::horizontal(Val::Px(12.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.10, 0.09, 0.07)),
                    BorderColor::from(BUTTON_BORDER),
                    BorderRadius::all(Val::Px(4.0)),
                ))
                .with_children(|toggle| {
                    // Selected name
                    toggle.spawn((
                        DropdownText,
                        Text::new(selected_name),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(TEXT_COLOR),
                    ));
                    
                    // Arrow indicator
                    toggle.spawn((
                        Text::new("▼"),
                        TextFont {
                            font_size: 12.0,
                            ..default()
                        },
                        TextColor(TEXT_MUTED),
                    ));
                });
            
            // Options container (hidden by default, shown when expanded)
            dropdown_container
                .spawn((
                    DropdownOptions,
                    Node {
                        flex_direction: FlexDirection::Column,
                        width: Val::Px(280.0),
                        border: UiRect::all(Val::Px(1.0)),
                        display: Display::None, // Hidden by default
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.10, 0.09, 0.07)),
                    BorderColor::from(BUTTON_BORDER),
                    BorderRadius::all(Val::Px(4.0)),
                    ZIndex(10), // On top of other elements
                ))
                .with_children(|options| {
                    for (i, entry) in presets.entries.iter().enumerate() {
                        let is_selected = presets.selected_index == Some(i);
                        
                        options
                            .spawn((
                                DropdownOption { index: i },
                                Button,
                                Node {
                                    width: Val::Percent(100.0),
                                    height: Val::Px(36.0),
                                    justify_content: JustifyContent::FlexStart,
                                    align_items: AlignItems::Center,
                                    padding: UiRect::horizontal(Val::Px(12.0)),
                                    ..default()
                                },
                                BackgroundColor(if is_selected {
                                    Color::srgb(0.18, 0.14, 0.10)
                                } else {
                                    Color::srgb(0.10, 0.09, 0.07)
                                }),
                            ))
                            .with_children(|option| {
                                option.spawn((
                                    Text::new(&entry.name),
                                    TextFont {
                                        font_size: 15.0,
                                        ..default()
                                    },
                                    TextColor(if is_selected { ACCENT_COLOR } else { TEXT_COLOR }),
                                ));
                            });
                    }
                });
        });
}

fn spawn_button(parent: &mut ChildSpawnerCommands<'_>, text: &str, action: MenuButton) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: Val::Px(280.0),
                height: Val::Px(55.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                margin: UiRect::all(Val::Px(8.0)),
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::from(BUTTON_BORDER),
            BorderRadius::all(Val::Px(6.0)),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(text),
                TextFont {
                    font_size: 22.0,
                    ..default()
                },
                TextColor(TEXT_COLOR),
            ));
        });
}

fn despawn_main_menu(mut commands: Commands, query: Query<Entity, With<MainMenuRoot>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

// =============================================================================
// INTERACTIONS
// =============================================================================

fn button_interactions(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (Changed<Interaction>, With<Button>, Without<IpInputField>, Without<DropdownToggle>, Without<DropdownOption>),
    >,
) {
    for (interaction, mut bg_color, mut border_color) in buttons.iter_mut() {
        match interaction {
            Interaction::Pressed => {
                *bg_color = BackgroundColor(BUTTON_PRESSED);
                *border_color = BorderColor::from(ACCENT_COLOR);
            }
            Interaction::Hovered => {
                *bg_color = BackgroundColor(BUTTON_HOVERED);
                *border_color = BorderColor::from(ACCENT_COLOR);
            }
            Interaction::None => {
                *bg_color = BackgroundColor(BUTTON_NORMAL);
                *border_color = BorderColor::from(BUTTON_BORDER);
            }
        };
    }
}

fn handle_menu_actions(
    buttons: Query<(&Interaction, &MenuButton), Changed<Interaction>>,
    mut next_state: ResMut<NextState<GameState>>,
    mut exit_writer: MessageWriter<AppExit>,
) {
    for (interaction, action) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            match action {
                MenuButton::Connect => {
                    info!("Connect pressed - transitioning to Connecting state");
                    next_state.set(GameState::Connecting);
                }
                MenuButton::Exit => {
                    info!("Exit pressed - quitting game");
                    exit_writer.write(AppExit::Success);
                }
            }
        }
    }
}

fn animate_logo(time: Res<Time>, mut logos: Query<(&mut LogoImage, &mut Transform)>) {
    for (mut logo, mut transform) in logos.iter_mut() {
        logo.time += time.delta_secs();
        // Subtle breathing animation
        let scale = logo.base_scale + (logo.time * 0.5).sin() * 0.015;
        transform.scale = Vec3::splat(scale);
    }
}

/// Handle clicking on the IP input field to focus/unfocus
fn handle_ip_input_focus(
    mut input_fields: Query<(&Interaction, &mut IpInputField, &mut BorderColor)>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut presets: ResMut<ServerPresets>,
) {
    let mut any_clicked = false;
    
    for (interaction, mut field, mut border) in input_fields.iter_mut() {
        if *interaction == Interaction::Pressed {
            field.focused = true;
            any_clicked = true;
            *border = BorderColor::from(ACCENT_COLOR);
            // When manually editing, deselect preset
            presets.selected_index = None;
        }
    }
    
    // Unfocus if clicked elsewhere
    if mouse_button.just_pressed(MouseButton::Left) && !any_clicked {
        for (_, mut field, mut border) in input_fields.iter_mut() {
            if field.focused {
                field.focused = false;
                *border = BorderColor::from(BUTTON_BORDER);
            }
        }
    }
}

/// Handle keyboard input when IP field is focused
fn handle_ip_keyboard_input(
    mut input_fields: Query<&mut IpInputField>,
    mut server_address: ResMut<ServerAddress>,
    mut keyboard_events: MessageReader<KeyboardInput>,
) {
    let Some(mut field) = input_fields.iter_mut().find(|f| f.focused) else {
        return;
    };
    
    for event in keyboard_events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        
        match &event.logical_key {
            Key::Backspace => {
                // Remove last character from IP
                if !server_address.ip.is_empty() {
                    server_address.ip.pop();
                }
            }
            Key::Escape | Key::Enter => {
                // Unfocus on escape or enter
                field.focused = false;
            }
            Key::Character(c) => {
                // Allow valid IP/hostname characters: alphanumeric, dots, dashes
                let c = c.as_str();
                if c.len() == 1 {
                    let ch = c.chars().next().unwrap();
                    if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' {
                        // Limit length to reasonable hostname
                        if server_address.ip.len() < 63 {
                            server_address.ip.push(ch);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Update the displayed IP text
fn update_ip_display(
    server_address: Res<ServerAddress>,
    input_fields: Query<&IpInputField>,
    mut text_query: Query<&mut Text, With<IpTextDisplay>>,
    time: Res<Time>,
    mut cursor_timer: Local<f32>,
) {
    let is_focused = input_fields.iter().any(|f| f.focused);
    
    *cursor_timer += time.delta_secs();
    let show_cursor = is_focused && (*cursor_timer % 1.0) < 0.5;
    
    for mut text in text_query.iter_mut() {
        let display = if server_address.ip.is_empty() {
            format!("_:{}", server_address.port)
        } else {
            format!("{}:{}", server_address.ip, server_address.port)
        };
        
        let cursor = if show_cursor { "│" } else { "" };
        **text = format!("{}{}", display, cursor);
    }
}

// =============================================================================
// DROPDOWN LOGIC
// =============================================================================

/// Toggle dropdown open/closed when clicking the toggle button
fn handle_dropdown_toggle(
    toggle_query: Query<&Interaction, (Changed<Interaction>, With<DropdownToggle>)>,
    mut dropdown_state: ResMut<DropdownState>,
    mut options_query: Query<&mut Node, With<DropdownOptions>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    option_interactions: Query<&Interaction, With<DropdownOption>>,
    toggle_interactions: Query<&Interaction, With<DropdownToggle>>,
) {
    // Toggle on click
    for interaction in toggle_query.iter() {
        if *interaction == Interaction::Pressed {
            dropdown_state.expanded = !dropdown_state.expanded;
        }
    }
    
    // Close dropdown when clicking outside (but not on options or toggle)
    if mouse_button.just_pressed(MouseButton::Left) {
        let clicking_option = option_interactions.iter().any(|i| *i == Interaction::Pressed);
        let clicking_toggle = toggle_interactions.iter().any(|i| *i == Interaction::Pressed);
        
        if dropdown_state.expanded && !clicking_option && !clicking_toggle {
            dropdown_state.expanded = false;
        }
    }
    
    // Update visibility
    for mut node in options_query.iter_mut() {
        node.display = if dropdown_state.expanded {
            Display::Flex
        } else {
            Display::None
        };
    }
}

/// Handle clicking on a dropdown option
fn handle_dropdown_selection(
    options_query: Query<(&Interaction, &DropdownOption), Changed<Interaction>>,
    mut server_address: ResMut<ServerAddress>,
    mut presets: ResMut<ServerPresets>,
    mut dropdown_state: ResMut<DropdownState>,
) {
    for (interaction, option) in options_query.iter() {
        if *interaction == Interaction::Pressed {
            if let Some(entry) = presets.entries.get(option.index).cloned() {
                // Update server address
                server_address.ip = entry.ip.clone();
                info!("Selected server preset: {} ({})", entry.name, entry.ip);
                
                // Update selected index
                presets.selected_index = Some(option.index);
                
                // Close dropdown
                dropdown_state.expanded = false;
            }
        }
    }
}

/// Update dropdown display text based on selection
fn update_dropdown_display(
    presets: Res<ServerPresets>,
    mut text_query: Query<&mut Text, With<DropdownText>>,
    mut options_query: Query<(&DropdownOption, &mut BackgroundColor, &Children)>,
    mut text_colors: Query<&mut TextColor>,
) {
    // Update toggle text
    let selected_name = presets.selected_index
        .and_then(|i| presets.entries.get(i))
        .map(|e| e.name.as_str())
        .unwrap_or("Custom");
    
    for mut text in text_query.iter_mut() {
        if **text != selected_name {
            **text = selected_name.to_string();
        }
    }
    
    // Update option highlighting
    for (option, mut bg, children) in options_query.iter_mut() {
        let is_selected = presets.selected_index == Some(option.index);
        
        *bg = BackgroundColor(if is_selected {
            Color::srgb(0.18, 0.14, 0.10)
        } else {
            Color::srgb(0.10, 0.09, 0.07)
        });
        
        // Update text color
        for child in children.iter() {
            if let Ok(mut color) = text_colors.get_mut(child) {
                *color = TextColor(if is_selected { ACCENT_COLOR } else { TEXT_COLOR });
            }
        }
    }
}
