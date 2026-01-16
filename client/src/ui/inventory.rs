//! Inventory UI - Valheim-style inventory grid
//!
//! Press I to open/close inventory.
//! Right-click slots to drop items.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use shared::{
    Inventory, LocalPlayer, INVENTORY_SLOTS, HOTBAR_SLOTS, CHEST_SLOTS,
    DropRequest, InventoryMoveRequest, HotbarSelection, ReliableChannel, ItemStack,
    ChestStorage, ChestTransferRequest,
};
use lightyear::prelude::*;
use lightyear::prelude::client::Connected;

use super::styles::*;
use crate::input::InputState;
use crate::chest::OpenChest;
use crate::states::GameState;

/// Plugin for inventory UI
pub struct InventoryPlugin;

impl Plugin for InventoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InventoryOpen>();
        app.init_resource::<DragState>();
        app.add_systems(Update, (
            toggle_inventory,
            spawn_inventory_ui,
            despawn_inventory_ui,
            // Chest interactions must run BEFORE inventory drag so chest clicks have priority
            handle_chest_slot_interactions,
            handle_drag_and_drop,
            update_inventory_slots,
            update_chest_slots,
            handle_slot_interactions,
        ).chain());
    }
}

/// Resource tracking if inventory is open
#[derive(Resource, Default)]
pub struct InventoryOpen(pub bool);

/// Marker for the inventory UI root
#[derive(Component)]
pub struct InventoryUI;

/// Marker for an inventory slot
#[derive(Component)]
pub struct InventorySlot {
    pub index: usize,
}

/// Marker for slot item icon
#[derive(Component)]
pub struct SlotIcon {
    pub index: usize,
}

/// Marker for slot quantity text
#[derive(Component)]
pub struct SlotQuantity {
    pub index: usize,
}

/// Marker for a chest slot (as opposed to player inventory slot)
#[derive(Component)]
pub struct ChestSlot {
    pub index: usize,
}

/// Marker for chest slot icon
#[derive(Component)]
pub struct ChestSlotIcon {
    pub index: usize,
}

/// Marker for chest slot quantity text
#[derive(Component)]
pub struct ChestSlotQuantity {
    pub index: usize,
}

/// Marker for the chest panel (so we can despawn it separately)
#[derive(Component)]
pub struct ChestPanel;

/// While dragging: which slot we started from + a floating icon under the cursor
#[derive(Resource, Default)]
pub struct DragState {
    pub dragging: bool,
    pub from_slot: Option<usize>,
    pub from_chest: bool, // true if dragging from chest, false if from inventory
    pub stack: Option<ItemStack>,
    pub icon_entity: Option<Entity>,
}

/// Marker for the floating drag icon UI
#[derive(Component)]
pub struct DragIcon;

/// Inventory slot colors
const SLOT_NORMAL: Color = Color::srgba(0.15, 0.12, 0.10, 0.9);
const SLOT_HOVERED: Color = Color::srgba(0.25, 0.20, 0.15, 0.95);
const SLOT_EMPTY: Color = Color::srgba(0.10, 0.08, 0.06, 0.7);
const SLOT_BORDER: Color = Color::srgba(0.4, 0.3, 0.2, 0.8);
const HOTBAR_BORDER: Color = Color::srgba(0.55, 0.45, 0.25, 0.9);

/// Toggle inventory open/close with I key
fn toggle_inventory(
    keyboard: Res<ButtonInput<KeyCode>>,
    game_state: Res<State<GameState>>,
    mut inventory_open: ResMut<InventoryOpen>,
    mut input_state: ResMut<InputState>,
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
) {
    // Only allow inventory toggle in Playing state
    if game_state.get() != &GameState::Playing {
        return;
    }

    if keyboard.just_pressed(KeyCode::KeyI) {
        inventory_open.0 = !inventory_open.0;
        input_state.inventory_open = inventory_open.0;
        
        // Show/hide cursor
        if let Ok(window_entity) = windows.single() {
            if let Ok(mut cursor) = cursor_opts.get_mut(window_entity) {
                if inventory_open.0 {
                    cursor.grab_mode = CursorGrabMode::None;
                    cursor.visible = true;
                } else {
                    cursor.grab_mode = CursorGrabMode::Locked;
                    cursor.visible = false;
                }
            }
        }
        
        info!("Inventory {}", if inventory_open.0 { "opened" } else { "closed" });
    }
    
    // Also close with Escape
    if keyboard.just_pressed(KeyCode::Escape) && inventory_open.0 {
        inventory_open.0 = false;
        input_state.inventory_open = false;
        if let Ok(window_entity) = windows.single() {
            if let Ok(mut cursor) = cursor_opts.get_mut(window_entity) {
                cursor.grab_mode = CursorGrabMode::Locked;
                cursor.visible = false;
            }
        }
    }
}

/// Spawn inventory UI when opened
fn spawn_inventory_ui(
    mut commands: Commands,
    inventory_open: Res<InventoryOpen>,
    open_chest: Res<OpenChest>,
    existing_ui: Query<Entity, With<InventoryUI>>,
) {
    // Only spawn if opened and not already existing
    if !inventory_open.0 || !existing_ui.is_empty() {
        return;
    }
    
    let chest_is_open = open_chest.entity.is_some();
    
    // Root container - centered on screen
    commands.spawn((
        InventoryUI,
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: Val::Px(20.0), // Space between panels
            ..default()
        },
        // Semi-transparent background
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
    )).with_children(|parent| {
        // Chest panel (only when chest is open) - LEFT side
        if chest_is_open {
            parent.spawn((
                ChestPanel,
                Node {
                    width: Val::Px(340.0),
                    height: Val::Px(180.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(16.0)),
                    border: UiRect::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(MENU_BACKGROUND),
                BorderColor::from(Color::srgb(0.5, 0.35, 0.2)), // Wood-ish
            )).with_children(|panel| {
                // Title
                panel.spawn((
                    Text::new("CHEST"),
                    TextFont {
                        font_size: 24.0,
                        ..default()
                    },
                    TextColor(Color::srgb(0.8, 0.6, 0.3)), // Golden-brown
                    Node {
                        margin: UiRect::bottom(Val::Px(12.0)),
                        ..default()
                    },
                ));
                
                // Chest grid (6 columns x 1 row = 6 slots)
                panel.spawn((
                    Node {
                        display: Display::Grid,
                        grid_template_columns: RepeatedGridTrack::flex(6, 1.0),
                        grid_template_rows: RepeatedGridTrack::flex(1, 1.0),
                        row_gap: Val::Px(6.0),
                        column_gap: Val::Px(6.0),
                        width: Val::Percent(100.0),
                        height: Val::Auto,
                        ..default()
                    },
                )).with_children(|grid| {
                    for i in 0..CHEST_SLOTS {
                        spawn_chest_slot(grid, i);
                    }
                });
            });
        }
        
        // Inventory panel - RIGHT side (or center if no chest)
        parent.spawn((
            Node {
                width: Val::Px(480.0),
                height: Val::Px(380.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(16.0)),
                border: UiRect::all(Val::Px(3.0)),
                ..default()
            },
            BackgroundColor(MENU_BACKGROUND),
            BorderColor::from(BUTTON_BORDER),
        )).with_children(|panel| {
            // Title
            panel.spawn((
                Text::new("INVENTORY"),
                TextFont {
                    font_size: 28.0,
                    ..default()
                },
                TextColor(ACCENT_COLOR),
                Node {
                    margin: UiRect::bottom(Val::Px(12.0)),
                    ..default()
                },
            ));
            
            // Grid container (6 columns x 4 rows = 24 slots)
            panel.spawn((
                Node {
                    display: Display::Grid,
                    grid_template_columns: RepeatedGridTrack::flex(6, 1.0),
                    grid_template_rows: RepeatedGridTrack::flex(4, 1.0),
                    row_gap: Val::Px(6.0),
                    column_gap: Val::Px(6.0),
                    width: Val::Percent(100.0),
                    height: Val::Auto,
                    ..default()
                },
            )).with_children(|grid| {
                for i in 0..INVENTORY_SLOTS {
                    spawn_slot(grid, i);
                }
            });
            
            // Instructions
            let hint = if chest_is_open {
                "Drag items between chest and inventory • Right-click to drop • Press E or ESC to close"
            } else {
                "Drag with left-click to move • Right-click to drop • Press I or ESC to close"
            };
            panel.spawn((
                Text::new(hint),
                TextFont {
                    font_size: 14.0,
                    ..default()
                },
                TextColor(TEXT_MUTED),
                Node {
                    margin: UiRect::top(Val::Px(12.0)),
                    ..default()
                },
            ));
        });
    });
}

/// Spawn a single chest slot
fn spawn_chest_slot(parent: &mut ChildSpawnerCommands, index: usize) {
    parent.spawn((
        ChestSlot { index },
        Button,
        Node {
            width: Val::Px(48.0),
            height: Val::Px(48.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(SLOT_EMPTY),
        BorderColor::from(Color::srgb(0.5, 0.35, 0.2)), // Wood-ish border
    )).with_children(|slot| {
        // Item icon (colored box)
        slot.spawn((
            ChestSlotIcon { index },
            Node {
                width: Val::Px(36.0),
                height: Val::Px(36.0),
                position_type: PositionType::Absolute,
                ..default()
            },
            BackgroundColor(Color::NONE),
        ));
        
        // Quantity text
        slot.spawn((
            ChestSlotQuantity { index },
            Text::new(""),
            TextFont {
                font_size: 14.0,
                ..default()
            },
            TextColor(TEXT_COLOR),
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(2.0),
                bottom: Val::Px(0.0),
                ..default()
            },
        ));
    });
}

/// Spawn a single inventory slot
fn spawn_slot(parent: &mut ChildSpawnerCommands, index: usize) {
    parent.spawn((
        InventorySlot { index },
        Button,
        Node {
            width: Val::Px(64.0),
            height: Val::Px(64.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(2.0)),
            ..default()
        },
        BackgroundColor(SLOT_EMPTY),
        BorderColor::from(SLOT_BORDER),
    )).with_children(|slot| {
        // Item icon (colored square)
        slot.spawn((
            SlotIcon { index },
            Node {
                width: Val::Px(46.0),
                height: Val::Px(46.0),
                position_type: PositionType::Absolute,
                ..default()
            },
            BackgroundColor(Color::NONE),
        ));
        
        // Quantity text (bottom-right corner)
        slot.spawn((
            SlotQuantity { index },
            Text::new(""),
            TextFont {
                font_size: 16.0,
                ..default()
            },
            TextColor(TEXT_COLOR),
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(2.0),
                right: Val::Px(4.0),
                ..default()
            },
        ));
    });
}

/// Despawn inventory UI when closed
fn despawn_inventory_ui(
    mut commands: Commands,
    inventory_open: Res<InventoryOpen>,
    ui_query: Query<Entity, With<InventoryUI>>,
) {
    if inventory_open.0 {
        return;
    }
    
    for entity in ui_query.iter() {
        commands.entity(entity).despawn();
    }
}

/// Handle left-click drag & drop within the inventory UI
fn handle_drag_and_drop(
    mut commands: Commands,
    inventory_open: Res<InventoryOpen>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    ui_root: Query<Entity, With<InventoryUI>>,
    slots: Query<(&InventorySlot, &Interaction), Without<ChestSlot>>,
    chest_slots: Query<(&ChestSlot, &Interaction), Without<InventorySlot>>,
    mut drag: ResMut<DragState>,
    mut local_player_inventory: Query<&mut Inventory, With<LocalPlayer>>,
    mut client_query: Query<&mut MessageSender<InventoryMoveRequest>, (With<crate::GameClient>, With<Connected>)>,
    mut drag_icon_nodes: Query<&mut Node, With<DragIcon>>,
    mut drag_icon_bg: Query<&mut BackgroundColor, With<DragIcon>>,
    mut drag_icon_text: Query<&mut Text, With<DragIcon>>,
) {
    if !inventory_open.0 {
        drag.dragging = false;
        drag.from_slot = None;
        drag.from_chest = false;
        drag.stack = None;
        drag.icon_entity = None;
        return;
    }
    
    let hovered_slot = slots
        .iter()
        .find(|(_, interaction)| **interaction == Interaction::Hovered || **interaction == Interaction::Pressed)
        .map(|(slot, _)| slot.index);
    
    // Check if hovering over a chest slot - if so, don't start an inventory drag
    let hovering_chest = chest_slots
        .iter()
        .any(|(_, interaction)| *interaction == Interaction::Hovered || *interaction == Interaction::Pressed);
    
    // Start drag (only from inventory slots, not chest)
    if !drag.dragging && mouse.just_pressed(MouseButton::Left) && !hovering_chest {
        let Some(from) = hovered_slot else { return };
        
        let Ok(inventory) = local_player_inventory.single_mut() else { return };
        if let Some(stack) = inventory.get_slot(from).copied() {
            drag.dragging = true;
            drag.from_slot = Some(from);
            drag.from_chest = false; // Dragging from inventory
            drag.stack = Some(stack);
            
            // Spawn floating icon under cursor (child of inventory root)
            if let Ok(root) = ui_root.single() {
                commands.entity(root).with_children(|parent| {
                    let ec = parent.spawn((
                        DragIcon,
                        Node {
                            position_type: PositionType::Absolute,
                            width: Val::Px(46.0),
                            height: Val::Px(46.0),
                            ..default()
                        },
                        BackgroundColor(stack.item_type.color()),
                        BorderColor::from(ACCENT_COLOR),
                        Text::new(if stack.quantity > 1 { format!("{}", stack.quantity) } else { String::new() }),
                        TextFont { font_size: 14.0, ..default() },
                        TextColor(TEXT_COLOR),
                    ));
                    drag.icon_entity = Some(ec.id());
                });
            }
        }
    }
    
    // Update drag icon position (only when dragging from inventory, not from chest)
    if drag.dragging && !drag.from_chest {
        if let Ok(window) = windows.single() {
            if let Some(cursor) = window.cursor_position() {
                let x = cursor.x - 23.0;
                let y = cursor.y - 23.0; // top property works from top down, same as cursor
                for mut node in drag_icon_nodes.iter_mut() {
                    node.left = Val::Px(x.max(0.0));
                    node.top = Val::Px(y.max(0.0));
                }
            }
        }
        // Keep icon color/text in sync (weapon/items)
        if let Some(stack) = drag.stack {
            for mut bg in drag_icon_bg.iter_mut() {
                *bg = BackgroundColor(stack.item_type.color());
            }
            for mut text in drag_icon_text.iter_mut() {
                **text = if stack.quantity > 1 { format!("{}", stack.quantity) } else { String::new() };
            }
        }
    }
    
    // End drag (only for inventory-to-inventory, chest drags are handled in handle_chest_slot_interactions)
    if drag.dragging && !drag.from_chest && mouse.just_released(MouseButton::Left) {
        let Some(from) = drag.from_slot else {
            drag.dragging = false;
            drag.from_chest = false;
            return;
        };
        
        // Check if dropping onto a chest slot (handled by chest system)
        let dropping_to_chest = chest_slots
            .iter()
            .any(|(_, interaction)| *interaction == Interaction::Hovered || *interaction == Interaction::Pressed);
        
        if !dropping_to_chest {
            // Inventory to inventory move
            if let Some(to) = hovered_slot {
                if to != from {
                    if let Ok(mut sender) = client_query.single_mut() {
                        let _ = sender.send::<ReliableChannel>(InventoryMoveRequest {
                            from: from as u8,
                            to: to as u8,
                        });
                    }
                    
                    // Client-side prediction for snappy UI
                    if let Ok(mut inv) = local_player_inventory.single_mut() {
                        let _ = inv.move_or_stack_slot(from, to);
                    }
                }
            }
            
            // Despawn drag icon
            if let Some(icon) = drag.icon_entity.take() {
                commands.entity(icon).despawn();
            }
            
            drag.dragging = false;
            drag.from_slot = None;
            drag.from_chest = false;
            drag.stack = None;
        }
        // If dropping to chest, let the chest handler deal with cleanup
    }
}

/// Update slot visuals based on inventory contents
fn update_inventory_slots(
    inventory_open: Res<InventoryOpen>,
    local_player: Query<&Inventory, With<LocalPlayer>>,
    hotbar: Query<&HotbarSelection, With<LocalPlayer>>,
    drag: Res<DragState>,
    mut slots: Query<(&InventorySlot, &mut BackgroundColor, &mut BorderColor, &Interaction)>,
    mut icons: Query<(&SlotIcon, &mut BackgroundColor), Without<InventorySlot>>,
    mut quantities: Query<(&SlotQuantity, &mut Text)>,
) {
    if !inventory_open.0 {
        return;
    }
    
    let Ok(inventory) = local_player.single() else {
        return;
    };
    let active_hotbar = hotbar.single().ok().map(|h| h.index as usize);
    
    // Update slot backgrounds based on interaction
    for (slot, mut bg, mut border, interaction) in slots.iter_mut() {
        let has_item = inventory.get_slot(slot.index).is_some();
        let is_hotbar = slot.index < HOTBAR_SLOTS;
        let is_active = active_hotbar == Some(slot.index);
        let is_drag_from = drag.dragging && drag.from_slot == Some(slot.index);
        
        *bg = match interaction {
            Interaction::Hovered | Interaction::Pressed => BackgroundColor(SLOT_HOVERED),
            Interaction::None => {
                if has_item {
                    BackgroundColor(SLOT_NORMAL)
                } else {
                    BackgroundColor(SLOT_EMPTY)
                }
            }
        };
        
        // Border highlights: hotbar row + active slot
        let border_color = if is_active {
            ACCENT_COLOR
        } else if is_drag_from {
            Color::srgba(1.0, 1.0, 1.0, 0.8)
        } else if is_hotbar {
            HOTBAR_BORDER
        } else {
            SLOT_BORDER
        };
        *border = BorderColor::from(border_color);
    }
    
    // Update icons
    for (icon, mut bg) in icons.iter_mut() {
        if let Some(stack) = inventory.get_slot(icon.index) {
            *bg = BackgroundColor(stack.item_type.color());
        } else {
            *bg = BackgroundColor(Color::NONE);
        }
    }
    
    // Update quantities
    for (qty, mut text) in quantities.iter_mut() {
        if let Some(stack) = inventory.get_slot(qty.index) {
            **text = format!("{}", stack.quantity);
        } else {
            **text = String::new();
        }
    }
}

/// Handle slot interactions (right-click to drop)
fn handle_slot_interactions(
    inventory_open: Res<InventoryOpen>,
    mouse: Res<ButtonInput<MouseButton>>,
    slots: Query<(&InventorySlot, &Interaction)>,
    local_player: Query<&Inventory, With<LocalPlayer>>,
    mut client_query: Query<&mut MessageSender<DropRequest>, (With<crate::GameClient>, With<Connected>)>,
) {
    if !inventory_open.0 {
        return;
    }
    
    // Check for right-click on slots
    if !mouse.just_pressed(MouseButton::Right) {
        return;
    }
    
    let Ok(inventory) = local_player.single() else {
        return;
    };
    
    for (slot, interaction) in slots.iter() {
        if *interaction == Interaction::Hovered || *interaction == Interaction::Pressed {
            // Check if slot has an item
            if inventory.get_slot(slot.index).is_some() {
                // Send drop request
                if let Ok(mut sender) = client_query.single_mut() {
                    let _ = sender.send::<ReliableChannel>(DropRequest { slot_index: slot.index });
                    info!("Requesting drop from slot {}", slot.index);
                }
            }
        }
    }
}

/// Update chest slot visuals based on chest contents
fn update_chest_slots(
    inventory_open: Res<InventoryOpen>,
    open_chest: Res<OpenChest>,
    chests: Query<&ChestStorage>,
    drag: Res<DragState>,
    mut slots: Query<(&ChestSlot, &mut BackgroundColor, &mut BorderColor, &Interaction)>,
    mut icons: Query<(&ChestSlotIcon, &mut BackgroundColor), Without<ChestSlot>>,
    mut quantities: Query<(&ChestSlotQuantity, &mut Text)>,
) {
    if !inventory_open.0 {
        return;
    }
    
    let Some(chest_entity) = open_chest.entity else {
        return;
    };
    
    let Ok(chest) = chests.get(chest_entity) else {
        return;
    };
    
    // Update slot backgrounds
    for (slot, mut bg, mut border, interaction) in slots.iter_mut() {
        let has_item = chest.get_slot(slot.index).is_some();
        let is_drag_from = drag.dragging && drag.from_chest && drag.from_slot == Some(slot.index);
        
        *bg = match interaction {
            Interaction::Hovered | Interaction::Pressed => BackgroundColor(SLOT_HOVERED),
            Interaction::None => {
                if has_item {
                    BackgroundColor(SLOT_NORMAL)
                } else {
                    BackgroundColor(SLOT_EMPTY)
                }
            }
        };
        
        // Border highlight when dragging from this slot
        let border_color = if is_drag_from {
            Color::srgba(1.0, 1.0, 1.0, 0.8)
        } else {
            Color::srgb(0.5, 0.35, 0.2)
        };
        *border = BorderColor::from(border_color);
    }
    
    // Update icons
    for (icon, mut bg) in icons.iter_mut() {
        if let Some(stack) = chest.get_slot(icon.index) {
            *bg = BackgroundColor(stack.item_type.color());
        } else {
            *bg = BackgroundColor(Color::NONE);
        }
    }
    
    // Update quantities
    for (qty, mut text) in quantities.iter_mut() {
        if let Some(stack) = chest.get_slot(qty.index) {
            **text = format!("{}", stack.quantity);
        } else {
            **text = String::new();
        }
    }
}

/// Handle chest slot interactions (drag to/from chest)
fn handle_chest_slot_interactions(
    mut commands: Commands,
    inventory_open: Res<InventoryOpen>,
    open_chest: Res<OpenChest>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    ui_root: Query<Entity, With<InventoryUI>>,
    inv_slots: Query<(&InventorySlot, &Interaction), Without<ChestSlot>>,
    chest_slots: Query<(&ChestSlot, &Interaction), Without<InventorySlot>>,
    mut drag: ResMut<DragState>,
    chests: Query<&ChestStorage>,
    local_player: Query<&Inventory, With<LocalPlayer>>,
    mut client_query: Query<&mut MessageSender<ChestTransferRequest>, (With<crate::GameClient>, With<Connected>)>,
    mut drag_icon_nodes: Query<&mut Node, With<DragIcon>>,
    mut drag_icon_bg: Query<&mut BackgroundColor, With<DragIcon>>,
    mut drag_icon_text: Query<&mut Text, With<DragIcon>>,
) {
    if !inventory_open.0 || open_chest.entity.is_none() {
        return;
    }
    
    let Some(chest_entity) = open_chest.entity else { return };
    let Ok(chest) = chests.get(chest_entity) else { return };
    let Ok(_inventory) = local_player.single() else { return };
    
    // Find hovered slots
    let hovered_chest_slot = chest_slots
        .iter()
        .find(|(_, i)| **i == Interaction::Hovered || **i == Interaction::Pressed)
        .map(|(s, _)| s.index);
    
    let hovered_inv_slot = inv_slots
        .iter()
        .find(|(_, i)| **i == Interaction::Hovered || **i == Interaction::Pressed)
        .map(|(s, _)| s.index);
    
    // Start drag from chest slot
    if !drag.dragging && mouse.just_pressed(MouseButton::Left) {
        if let Some(from) = hovered_chest_slot {
            if let Some(stack) = chest.get_slot(from).copied() {
                drag.dragging = true;
                drag.from_slot = Some(from);
                drag.from_chest = true;
                drag.stack = Some(stack);
                
                // Spawn floating icon
                if let Ok(root) = ui_root.single() {
                    commands.entity(root).with_children(|parent| {
                        let ec = parent.spawn((
                            DragIcon,
                            Node {
                                position_type: PositionType::Absolute,
                                width: Val::Px(46.0),
                                height: Val::Px(46.0),
                                ..default()
                            },
                            BackgroundColor(stack.item_type.color()),
                            BorderColor::from(ACCENT_COLOR),
                            Text::new(if stack.quantity > 1 { format!("{}", stack.quantity) } else { String::new() }),
                            TextFont { font_size: 14.0, ..default() },
                            TextColor(TEXT_COLOR),
                        ));
                        drag.icon_entity = Some(ec.id());
                    });
                }
            }
        }
    }
    
    // Update drag icon position while dragging from chest
    if drag.dragging && drag.from_chest {
        if let Ok(window) = windows.single() {
            if let Some(cursor) = window.cursor_position() {
                let x = cursor.x - 23.0;
                let y = cursor.y - 23.0;
                for mut node in drag_icon_nodes.iter_mut() {
                    node.left = Val::Px(x.max(0.0));
                    node.top = Val::Px(y.max(0.0));
                }
            }
        }
        if let Some(stack) = drag.stack {
            for mut bg in drag_icon_bg.iter_mut() {
                *bg = BackgroundColor(stack.item_type.color());
            }
            for mut text in drag_icon_text.iter_mut() {
                **text = if stack.quantity > 1 { format!("{}", stack.quantity) } else { String::new() };
            }
        }
    }
    
    // End drag - only handle chest-related transfers
    // (inventory -> inventory is handled by handle_drag_and_drop)
    if drag.dragging && mouse.just_released(MouseButton::Left) {
        let from_chest = drag.from_chest;
        let from_slot = drag.from_slot;
        
        // Only handle if:
        // 1. Dragging FROM chest (chest -> anywhere), or
        // 2. Dragging TO chest (inventory -> chest)
        let should_handle = from_chest || hovered_chest_slot.is_some();
        
        if !should_handle {
            // Not a chest-related transfer, let handle_drag_and_drop deal with it
            return;
        }
        
        // Clean up drag state
        if let Some(icon) = drag.icon_entity.take() {
            commands.entity(icon).despawn();
        }
        drag.dragging = false;
        drag.from_slot = None;
        drag.from_chest = false;
        drag.stack = None;
        
        let Some(from_slot) = from_slot else { return };
        
        // Determine target and send transfer request
        if from_chest {
            // Dragging FROM chest
            if let Some(to_slot) = hovered_inv_slot {
                // Chest -> Inventory
                if let Ok(mut sender) = client_query.single_mut() {
                    let _ = sender.send::<ReliableChannel>(ChestTransferRequest {
                        from_chest: true,
                        from_slot: from_slot as u8,
                        to_slot: to_slot as u8,
                    });
                }
            } else if let Some(to_slot) = hovered_chest_slot {
                // Chest -> Chest (reordering within chest)
                if to_slot != from_slot {
                    // For now, just ignore - can't reorder within chest
                }
            }
        } else {
            // Dragging FROM inventory TO chest
            if let Some(to_slot) = hovered_chest_slot {
                if let Ok(mut sender) = client_query.single_mut() {
                    let _ = sender.send::<ReliableChannel>(ChestTransferRequest {
                        from_chest: false,
                        from_slot: from_slot as u8,
                        to_slot: to_slot as u8,
                    });
                }
            }
        }
    }
}
