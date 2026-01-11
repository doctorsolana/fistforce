//! Pause menu UI (in-game escape menu)
//!
//! Updated for Bevy 0.17 / Lightyear 0.25

use bevy::prelude::*;
use bevy::app::AppExit;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use lightyear::prelude::client::*;

use crate::states::GameState;
use crate::GameClient;
use super::styles::*;

pub struct PauseMenuPlugin;

impl Plugin for PauseMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(GameState::Paused), spawn_pause_menu);
        app.add_systems(OnExit(GameState::Paused), despawn_pause_menu);
        app.add_systems(
            Update,
            (button_interactions, handle_pause_actions).run_if(in_state(GameState::Paused)),
        );
        app.add_systems(Update, handle_escape_key.run_if(in_state(GameState::Playing)));
        app.add_systems(Update, handle_resume_key.run_if(in_state(GameState::Paused)));
        
        // Release cursor when entering pause
        app.add_systems(OnEnter(GameState::Paused), release_cursor);
    }
}

/// Marker for the pause menu root
#[derive(Component)]
struct PauseMenuRoot;

/// Pause menu button actions
#[derive(Component, Clone, Copy)]
enum PauseButton {
    Resume,
    Disconnect,
    Exit,
}

fn spawn_pause_menu(mut commands: Commands) {
    commands
        .spawn((
            PauseMenuRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.7)),
        ))
        .with_children(|parent| {
            // Pause title
            parent.spawn((
                Text::new("PAUSED"),
                title_text_style(),
                TextColor(TEXT_COLOR),
                Node {
                    margin: UiRect::bottom(Val::Px(40.0)),
                    ..default()
                },
            ));

            // Resume button
            spawn_button(parent, "RESUME", PauseButton::Resume);

            // Disconnect button
            spawn_button(parent, "DISCONNECT", PauseButton::Disconnect);

            // Exit button
            spawn_button(parent, "EXIT GAME", PauseButton::Exit);

            // Hint
            parent.spawn((
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

fn despawn_pause_menu(mut commands: Commands, query: Query<Entity, With<PauseMenuRoot>>) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}

fn button_interactions(
    mut buttons: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<Button>)>,
) {
    for (interaction, mut bg_color) in buttons.iter_mut() {
        *bg_color = match interaction {
            Interaction::Pressed => BackgroundColor(BUTTON_PRESSED),
            Interaction::Hovered => BackgroundColor(BUTTON_HOVERED),
            Interaction::None => BackgroundColor(BUTTON_NORMAL),
        };
    }
}

fn handle_pause_actions(
    buttons: Query<(&Interaction, &PauseButton), Changed<Interaction>>,
    mut next_state: ResMut<NextState<GameState>>,
    mut exit: MessageWriter<AppExit>,
    mut commands: Commands,
    client_query: Query<Entity, With<GameClient>>,
) {
    for (interaction, action) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            match action {
                PauseButton::Resume => {
                    next_state.set(GameState::Playing);
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
