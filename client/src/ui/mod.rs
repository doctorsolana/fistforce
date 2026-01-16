//! UI module

pub mod main_menu;
pub mod pause_menu;
pub mod inventory;
pub mod name_entry;
pub mod styles;

pub use main_menu::MainMenuPlugin;
pub use main_menu::ServerAddress;
pub use pause_menu::PauseMenuPlugin;
pub use inventory::InventoryPlugin;
pub use name_entry::NameEntryPlugin;
