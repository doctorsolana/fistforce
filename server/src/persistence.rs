//! Player persistence - disk I/O for player profiles
//!
//! Handles loading and saving player profiles to disk using bincode serialization.
//! Uses atomic writes (temp file + rename) to prevent corruption.

use std::collections::HashMap;
use std::path::PathBuf;
use bevy::prelude::*;
use lightyear::prelude::PeerId;
use shared::{PlayerProfile, PROFILE_VERSION, NameRejectionReason};

/// Resource managing player profile persistence
#[derive(Resource)]
pub struct PlayerProfiles {
    /// Active profiles for currently connected players (lowercase name → profile)
    pub profiles: HashMap<String, PlayerProfile>,

    /// Directory where profile files are stored
    pub storage_dir: PathBuf,

    /// Bidirectional mapping for quick lookup
    /// PeerId → lowercase player name
    pub peer_to_name: HashMap<PeerId, String>,
    /// Lowercase player name → PeerId
    pub name_to_peer: HashMap<String, PeerId>,
}

impl PlayerProfiles {
    /// Create new PlayerProfiles resource with the specified storage directory
    pub fn new(storage_dir: PathBuf) -> Self {
        // Create directory if it doesn't exist
        std::fs::create_dir_all(&storage_dir)
            .expect("Failed to create player storage directory");

        info!("Player profiles will be saved to: {:?}", storage_dir);

        Self {
            profiles: HashMap::new(),
            storage_dir,
            peer_to_name: HashMap::new(),
            name_to_peer: HashMap::new(),
        }
    }

    /// Load a player profile from disk
    ///
    /// Returns:
    /// - Ok(profile) if file exists and is valid
    /// - Err(message) if file doesn't exist or is corrupted/wrong version
    pub fn load_profile(&self, name: &str) -> Result<PlayerProfile, String> {
        let name_lower = name.to_lowercase();
        let path = self.storage_dir.join(format!("{}.bin", name_lower));

        if !path.exists() {
            return Err(format!("Profile '{}' not found", name));
        }

        // Read file
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

        // Deserialize
        let profile: PlayerProfile = bincode::deserialize(&bytes)
            .map_err(|e| format!("Failed to deserialize {}: {}", path.display(), e))?;

        // Check version
        if profile.version != PROFILE_VERSION {
            // Backup old version
            let backup_path = self.storage_dir.join(format!("{}.v{}.backup", name_lower, profile.version));
            if let Err(e) = std::fs::copy(&path, &backup_path) {
                warn!("Failed to backup old profile version: {}", e);
            }

            return Err(format!(
                "Profile version mismatch: found v{}, expected v{}. Backed up to {:?}",
                profile.version, PROFILE_VERSION, backup_path
            ));
        }

        Ok(profile)
    }

    /// Save a player profile to disk (atomic write via temp file)
    ///
    /// This uses a temporary file + rename to ensure atomic writes and prevent
    /// corruption if the server crashes mid-write.
    pub fn save_profile(&self, profile: &PlayerProfile) -> Result<(), String> {
        let name_lower = profile.player_name.to_lowercase();
        let final_path = self.storage_dir.join(format!("{}.bin", name_lower));
        let temp_path = self.storage_dir.join(format!("{}.tmp", name_lower));

        // Serialize
        let bytes = bincode::serialize(profile)
            .map_err(|e| format!("Serialize error: {}", e))?;

        // Write to temp file
        std::fs::write(&temp_path, &bytes)
            .map_err(|e| format!("Write temp file error: {}", e))?;

        // Atomic rename (this is atomic on most filesystems)
        std::fs::rename(&temp_path, &final_path)
            .map_err(|e| format!("Rename error: {}", e))?;

        info!("Saved profile: {} ({} bytes)", profile.player_name, bytes.len());
        Ok(())
    }

    /// Validate a player name
    ///
    /// Returns:
    /// - Ok(()) if name is valid
    /// - Err(reason) if name is invalid
    pub fn validate_name(name: &str) -> Result<(), NameRejectionReason> {
        let trimmed = name.trim();

        // Length checks
        if trimmed.len() < 3 {
            return Err(NameRejectionReason::TooShort);
        }
        if trimmed.len() > 16 {
            return Err(NameRejectionReason::TooLong);
        }

        // Character validation: only alphanumeric, underscore, hyphen
        if !trimmed.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
            return Err(NameRejectionReason::InvalidCharacters);
        }

        // Reserved names (case-insensitive)
        let lower = trimmed.to_lowercase();
        const RESERVED: &[&str] = &["server", "admin", "system", "moderator", "mod", "bot", "console"];
        if RESERVED.contains(&lower.as_str()) {
            return Err(NameRejectionReason::Reserved);
        }

        Ok(())
    }

    /// Check if a name is currently in use by a connected player
    pub fn is_name_online(&self, name: &str) -> bool {
        self.name_to_peer.contains_key(&name.to_lowercase())
    }
}
