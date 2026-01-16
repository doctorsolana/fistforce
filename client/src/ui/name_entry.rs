//! Player name entry UI
//!
//! Simple name input screen shown when connected to server

use bevy::prelude::*;
use bevy::input::keyboard::KeyboardInput;
use lightyear::prelude::*;
use shared::{SubmitPlayerName, NameSubmissionResult, NameRejectionReason, ReliableChannel};

use crate::states::GameState;

// UI colors
const TEXT_COLOR: Color = Color::srgb(0.9, 0.9, 0.9);
const INPUT_BG: Color = Color::srgb(0.15, 0.15, 0.15);
const BUTTON_NORMAL: Color = Color::srgb(0.25, 0.55, 0.35);
const BUTTON_HOVERED: Color = Color::srgb(0.3, 0.65, 0.45);
const BUTTON_PRESSED: Color = Color::srgb(0.35, 0.75, 0.55);
const ERROR_COLOR: Color = Color::srgb(0.9, 0.3, 0.3);

/// Plugin for name entry UI
pub struct NameEntryPlugin;

impl Plugin for NameEntryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PlayerNameInput>();
        app.init_resource::<NameSubmissionFeedback>();

        app.add_systems(OnEnter(GameState::Connected), spawn_name_entry_ui);
        app.add_systems(OnExit(GameState::Connected), despawn_name_entry_ui);

        app.add_systems(
            Update,
            (
                handle_text_input,
                handle_submit_button,
                handle_enter_key_submit,
                handle_name_submission_result,
            )
                .run_if(in_state(GameState::Connected)),
        );
    }
}

/// Resource holding the player's name input
#[derive(Resource, Default)]
pub struct PlayerNameInput {
    pub name: String,
    pub submitted: bool,
}

/// Resource for feedback messages (errors, etc.)
#[derive(Resource, Default)]
pub struct NameSubmissionFeedback {
    pub error_message: Option<String>,
}

/// Root marker for the name entry UI
#[derive(Component)]
struct NameEntryRoot;

/// Marker for the name input text display
#[derive(Component)]
struct NameInputDisplay;

/// Marker for the error message text
#[derive(Component)]
struct ErrorMessageText;

/// Marker for the submit button
#[derive(Component)]
struct SubmitButton;

fn spawn_name_entry_ui(mut commands: Commands, mut name_input: ResMut<PlayerNameInput>) {
    // Reset input state
    name_input.name.clear();
    name_input.submitted = false;

    commands
        .spawn((
            NameEntryRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
        ))
        .with_children(|root| {
            // Panel
            root.spawn(Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(40.0)),
                row_gap: Val::Px(20.0),
                ..default()
            })
            .with_children(|panel| {
                // Title
                panel.spawn(Node {
                    margin: UiRect::bottom(Val::Px(20.0)),
                    ..default()
                })
                .insert(Text::new("Enter Player Name"))
                .insert(TextFont {
                    font_size: 32.0,
                    ..default()
                })
                .insert(TextColor(TEXT_COLOR));

                // Input field
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        ..default()
                    })
                    .with_children(|input_col| {
                        input_col.spawn(Text::new("Name:"))
                            .insert(TextFont {
                                font_size: 18.0,
                                ..default()
                            })
                            .insert(TextColor(TEXT_COLOR));

                        input_col
                            .spawn((
                                Node {
                                    width: Val::Px(300.0),
                                    height: Val::Px(40.0),
                                    padding: UiRect::all(Val::Px(10.0)),
                                    ..default()
                                },
                                BackgroundColor(INPUT_BG),
                                BorderRadius::all(Val::Px(4.0)),
                            ))
                            .with_children(|input_box| {
                                input_box.spawn(NameInputDisplay)
                                    .insert(Text::new(""))
                                    .insert(TextFont {
                                        font_size: 18.0,
                                        ..default()
                                    })
                                    .insert(TextColor(TEXT_COLOR));
                            });

                        // Helper text
                        input_col.spawn(Text::new("3-16 characters, alphanumeric only"))
                            .insert(TextFont {
                                font_size: 12.0,
                                ..default()
                            })
                            .insert(TextColor(Color::srgba(0.7, 0.7, 0.7, 0.8)));
                    });

                // Error message (initially hidden)
                panel.spawn(ErrorMessageText)
                    .insert(Text::new(""))
                    .insert(TextFont {
                        font_size: 14.0,
                        ..default()
                    })
                    .insert(TextColor(ERROR_COLOR))
                    .insert(Node {
                        min_height: Val::Px(20.0),
                        ..default()
                    });

                // Submit button
                panel
                    .spawn((
                        SubmitButton,
                        Button,
                        Node {
                            width: Val::Px(150.0),
                            height: Val::Px(45.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            margin: UiRect::top(Val::Px(10.0)),
                            ..default()
                        },
                        BackgroundColor(BUTTON_NORMAL),
                        BorderRadius::all(Val::Px(4.0)),
                    ))
                    .with_children(|btn| {
                        btn.spawn(Text::new("Join Game"))
                            .insert(TextFont {
                                font_size: 20.0,
                                ..default()
                            })
                            .insert(TextColor(TEXT_COLOR));
                    });
            });
        });
}

fn despawn_name_entry_ui(mut commands: Commands, query: Query<Entity, With<NameEntryRoot>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

fn handle_text_input(
    mut name_input: ResMut<PlayerNameInput>,
    mut key_events: MessageReader<KeyboardInput>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut display_query: Query<&mut Text, With<NameInputDisplay>>,
) {
    // Don't accept input if already submitted
    if name_input.submitted {
        return;
    }

    // Handle backspace
    if keyboard.just_pressed(KeyCode::Backspace) {
        name_input.name.pop();
    }

    // Handle character input from keyboard events
    for event in key_events.read() {
        if !event.state.is_pressed() {
            continue;
        }

        // Convert KeyCode to character
        let c_opt = match event.key_code {
            KeyCode::KeyA => Some('a'),
            KeyCode::KeyB => Some('b'),
            KeyCode::KeyC => Some('c'),
            KeyCode::KeyD => Some('d'),
            KeyCode::KeyE => Some('e'),
            KeyCode::KeyF => Some('f'),
            KeyCode::KeyG => Some('g'),
            KeyCode::KeyH => Some('h'),
            KeyCode::KeyI => Some('i'),
            KeyCode::KeyJ => Some('j'),
            KeyCode::KeyK => Some('k'),
            KeyCode::KeyL => Some('l'),
            KeyCode::KeyM => Some('m'),
            KeyCode::KeyN => Some('n'),
            KeyCode::KeyO => Some('o'),
            KeyCode::KeyP => Some('p'),
            KeyCode::KeyQ => Some('q'),
            KeyCode::KeyR => Some('r'),
            KeyCode::KeyS => Some('s'),
            KeyCode::KeyT => Some('t'),
            KeyCode::KeyU => Some('u'),
            KeyCode::KeyV => Some('v'),
            KeyCode::KeyW => Some('w'),
            KeyCode::KeyX => Some('x'),
            KeyCode::KeyY => Some('y'),
            KeyCode::KeyZ => Some('z'),
            KeyCode::Digit0 => Some('0'),
            KeyCode::Digit1 => Some('1'),
            KeyCode::Digit2 => Some('2'),
            KeyCode::Digit3 => Some('3'),
            KeyCode::Digit4 => Some('4'),
            KeyCode::Digit5 => Some('5'),
            KeyCode::Digit6 => Some('6'),
            KeyCode::Digit7 => Some('7'),
            KeyCode::Digit8 => Some('8'),
            KeyCode::Digit9 => Some('9'),
            KeyCode::Minus => Some('-'),
            _ => None,
        };

        if let Some(c) = c_opt {
            // Limit to 16 characters
            if name_input.name.len() < 16 {
                name_input.name.push(c);
            }
        }
    }

    // Update display
    for mut text in display_query.iter_mut() {
        text.0 = if name_input.name.is_empty() {
            "_".to_string()
        } else {
            format!("{}_", name_input.name)
        };
    }
}

fn handle_submit_button(
    mut interaction_query: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<SubmitButton>)>,
    name_input: Res<PlayerNameInput>,
    mut feedback: ResMut<NameSubmissionFeedback>,
    client_query: Query<(Entity, &MessageSender<SubmitPlayerName>), With<crate::GameClient>>,
    mut error_text_query: Query<&mut Text, With<ErrorMessageText>>,
    mut commands: Commands,
) {
    for (interaction, mut bg_color) in interaction_query.iter_mut() {
        *bg_color = match interaction {
            Interaction::Pressed => {
                // Submit name when button is pressed
                submit_name(&name_input, &mut feedback, client_query, &mut error_text_query, &mut commands);
                BackgroundColor(BUTTON_PRESSED)
            }
            Interaction::Hovered => BackgroundColor(BUTTON_HOVERED),
            Interaction::None => BackgroundColor(BUTTON_NORMAL),
        };
    }
}

fn handle_enter_key_submit(
    keyboard: Res<ButtonInput<KeyCode>>,
    name_input: Res<PlayerNameInput>,
    mut feedback: ResMut<NameSubmissionFeedback>,
    client_query: Query<(Entity, &MessageSender<SubmitPlayerName>), With<crate::GameClient>>,
    mut error_text_query: Query<&mut Text, With<ErrorMessageText>>,
    mut commands: Commands,
) {
    if !keyboard.just_pressed(KeyCode::Enter) {
        return;
    }

    submit_name(&name_input, &mut feedback, client_query, &mut error_text_query, &mut commands);
}

fn submit_name(
    name_input: &PlayerNameInput,
    feedback: &mut NameSubmissionFeedback,
    client_query: Query<(Entity, &MessageSender<SubmitPlayerName>), With<crate::GameClient>>,
    error_text_query: &mut Query<&mut Text, With<ErrorMessageText>>,
    commands: &mut Commands,
) {
    // Don't submit if already submitted
    if name_input.submitted {
        return;
    }

    let name = name_input.name.trim();

    // Basic validation
    if name.len() < 3 {
        let error_msg = "Name must be at least 3 characters".to_string();
        feedback.error_message = Some(error_msg.clone());
        for mut text in error_text_query.iter_mut() {
            text.0 = error_msg.clone();
        }
        return;
    }
    if name.len() > 16 {
        let error_msg = "Name must be at most 16 characters".to_string();
        feedback.error_message = Some(error_msg.clone());
        for mut text in error_text_query.iter_mut() {
            text.0 = error_msg.clone();
        }
        return;
    }

    // Send to server
    let Ok((client_entity, _sender)) = client_query.single() else {
        warn!("No client entity found - cannot submit name");
        return;
    };

    info!("Submitting player name: '{}'", name);

    // Clone the name for the closure
    let name_clone = name.to_string();

    // Send message using commands queue
    commands.queue(move |world: &mut World| {
        if let Some(mut sender) = world.get_mut::<MessageSender<SubmitPlayerName>>(client_entity) {
            sender.send::<ReliableChannel>(SubmitPlayerName {
                name: name_clone,
            });
        }
    });

    // Mark as submitted
    commands.entity(client_entity).insert(PlayerNameSubmitted);

    // Clear error
    feedback.error_message = None;
    for mut text in error_text_query.iter_mut() {
        text.0 = String::new();
    }
}

/// Marker component to track that we've submitted a name
#[derive(Component)]
struct PlayerNameSubmitted;

fn handle_name_submission_result(
    mut next_state: ResMut<NextState<GameState>>,
    mut feedback: ResMut<NameSubmissionFeedback>,
    mut client_query: Query<(Entity, &mut MessageReceiver<NameSubmissionResult>), (With<crate::GameClient>, With<PlayerNameSubmitted>)>,
    mut error_text_query: Query<&mut Text, With<ErrorMessageText>>,
    mut commands: Commands,
) {
    let Ok((client_entity, mut receiver)) = client_query.single_mut() else {
        return;
    };

    for result in receiver.receive() {
        match result {
            NameSubmissionResult::Accepted { profile_loaded } => {
                if profile_loaded {
                    info!("Name accepted! Loaded existing profile");
                } else {
                    info!("Name accepted! Created new profile");
                }

                // Clear the submitted marker
                commands.entity(client_entity).remove::<PlayerNameSubmitted>();

                // Transition to Playing state
                next_state.set(GameState::Playing);
            }
            NameSubmissionResult::Rejected { reason } => {
                warn!("Name rejected: {:?}", reason);

                // Clear the submitted marker so user can try again
                commands.entity(client_entity).remove::<PlayerNameSubmitted>();

                // Show error message
                let error_msg = match reason {
                    NameRejectionReason::InvalidCharacters => "Name contains invalid characters".to_string(),
                    NameRejectionReason::TooShort => "Name is too short (min 3 characters)".to_string(),
                    NameRejectionReason::TooLong => "Name is too long (max 16 characters)".to_string(),
                    NameRejectionReason::Reserved => "This name is reserved".to_string(),
                    NameRejectionReason::AlreadyOnline => "This name is already in use".to_string(),
                };

                feedback.error_message = Some(error_msg.clone());

                // Update error text
                for mut text in error_text_query.iter_mut() {
                    text.0 = error_msg.clone();
                }
            }
        }
    }
}
