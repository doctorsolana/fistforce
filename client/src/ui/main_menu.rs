//! Main menu UI
//!
//! Updated for Bevy 0.17

use bevy::prelude::*;
use bevy::app::AppExit;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;

use crate::states::GameState;
use super::styles::*;
use shared::SERVER_PORT;

pub struct MainMenuPlugin;

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        // Initialize server address with default
        app.init_resource::<ServerAddress>();
        
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
            ).run_if(in_state(GameState::MainMenu)),
        );
    }
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

fn spawn_main_menu(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    server_address: Res<ServerAddress>,
) {
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
                    
                    // Helper text
                    ip_section.spawn((
                        Text::new("Click to edit • Use your LAN IP for multiplayer"),
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

fn spawn_button(parent: &mut bevy::ecs::hierarchy::ChildSpawnerCommands<'_>, text: &str, action: MenuButton) {
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

fn button_interactions(
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor, &mut BorderColor),
        (Changed<Interaction>, With<Button>),
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
) {
    let mut any_clicked = false;
    
    for (interaction, mut field, mut border) in input_fields.iter_mut() {
        if *interaction == Interaction::Pressed {
            field.focused = true;
            any_clicked = true;
            *border = BorderColor::from(ACCENT_COLOR);
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
                // Only allow valid IP characters: digits and dots
                let c = c.as_str();
                if c.len() == 1 {
                    let ch = c.chars().next().unwrap();
                    if ch.is_ascii_digit() || ch == '.' {
                        // Limit length to reasonable IP address
                        if server_address.ip.len() < 15 {
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
