//! Deterministic prop placement shared between client and server.
//!
//! The client uses this to spawn visual scenes.
//! The server will use the same generated `PropSpawn`s to create static colliders.

use bevy::prelude::*;
use noise::{NoiseFn, Perlin};

use crate::terrain::{Biome, ChunkCoord, TerrainGenerator, CHUNK_SIZE, WORLD_SEED};

/// Per-prop render tuning (client uses this to disable shadows / add culling).
#[derive(Component, Clone, Copy, Debug)]
pub struct PropRenderTuning {
    /// If false, meshes under this prop should be marked `NotShadowCaster` (client-side).
    pub casts_shadows: bool,
    /// If set, meshes under this prop should get a `VisibilityRange` (client-side).
    pub visible_end_distance: Option<f32>,
}

/// Stable identifier for a prop scene / asset type.
///
/// NOTE: We intentionally keep readable, source-derived names (with underscores) to
/// match asset naming. Stable external IDs are provided via [`PropKind::id`].
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropKind {
    // --- KayKit (Assetsfromassetpack) ---
    KayKit_Rock_1_A,
    KayKit_Rock_1_B,
    KayKit_Rock_1_C,
    KayKit_Rock_2_A,
    KayKit_Rock_2_B,
    KayKit_Rock_3_A,
    KayKit_Rock_3_B,
    KayKit_Rock_3_C,

    KayKit_Tree_Bare_1_A,
    KayKit_Tree_Bare_1_B,
    KayKit_Tree_Bare_2_A,
    KayKit_Tree_Bare_2_B,

    KayKit_Tree_1_A,
    KayKit_Tree_1_B,
    KayKit_Tree_2_A,
    KayKit_Tree_2_B,
    KayKit_Tree_3_A,
    KayKit_Tree_4_A,

    KayKit_Bush_1_A,
    KayKit_Bush_1_B,
    KayKit_Bush_2_A,
    KayKit_Bush_2_B,
    KayKit_Bush_3_A,
    KayKit_Bush_4_A,

    KayKit_Grass_1_A,
    KayKit_Grass_1_B,
    KayKit_Grass_2_A,
    KayKit_Grass_2_B,

    // --- Stylized Nature (StylizedNature) ---
    Nature_Pine_1,
    Nature_Pine_2,
    Nature_Pine_3,
    Nature_Pine_4,
    Nature_Pine_5,

    Nature_TwistedTree_1,
    Nature_TwistedTree_2,
    Nature_TwistedTree_3,
    Nature_TwistedTree_4,
    Nature_TwistedTree_5,
    Nature_CommonTree_1,
    Nature_CommonTree_2,
    Nature_CommonTree_3,

    Nature_Bush_Common,
    Nature_Bush_Common_Flowers,

    Nature_Mushroom_Common,
    Nature_Mushroom_Laetiporus,

    Nature_Fern_1,
    Nature_Plant_1,
    Nature_Plant_1_Big,
    Nature_Plant_7,

    Nature_Flower_3_Group,
    Nature_Flower_4_Group,
    Nature_Clover_1,
    Nature_Clover_2,

    Nature_Rock_Medium_1,
    Nature_Rock_Medium_2,
    Nature_Rock_Medium_3,
    Nature_Pebble_Round_1,
    Nature_Pebble_Round_2,

    Nature_Grass_Common_Short,
    Nature_Grass_Common_Tall,
    Nature_Grass_Wispy_Short,
    Nature_Grass_Wispy_Tall,
}

impl PropKind {
    /// Stable string id used by the collider bake manifest / database.
    pub const fn id(&self) -> &'static str {
        match self {
            // KayKit rocks
            PropKind::KayKit_Rock_1_A => "kaykit_rock_1_a",
            PropKind::KayKit_Rock_1_B => "kaykit_rock_1_b",
            PropKind::KayKit_Rock_1_C => "kaykit_rock_1_c",
            PropKind::KayKit_Rock_2_A => "kaykit_rock_2_a",
            PropKind::KayKit_Rock_2_B => "kaykit_rock_2_b",
            PropKind::KayKit_Rock_3_A => "kaykit_rock_3_a",
            PropKind::KayKit_Rock_3_B => "kaykit_rock_3_b",
            PropKind::KayKit_Rock_3_C => "kaykit_rock_3_c",

            // KayKit dead trees
            PropKind::KayKit_Tree_Bare_1_A => "kaykit_tree_bare_1_a",
            PropKind::KayKit_Tree_Bare_1_B => "kaykit_tree_bare_1_b",
            PropKind::KayKit_Tree_Bare_2_A => "kaykit_tree_bare_2_a",
            PropKind::KayKit_Tree_Bare_2_B => "kaykit_tree_bare_2_b",

            // KayKit trees
            PropKind::KayKit_Tree_1_A => "kaykit_tree_1_a",
            PropKind::KayKit_Tree_1_B => "kaykit_tree_1_b",
            PropKind::KayKit_Tree_2_A => "kaykit_tree_2_a",
            PropKind::KayKit_Tree_2_B => "kaykit_tree_2_b",
            PropKind::KayKit_Tree_3_A => "kaykit_tree_3_a",
            PropKind::KayKit_Tree_4_A => "kaykit_tree_4_a",

            // KayKit bushes
            PropKind::KayKit_Bush_1_A => "kaykit_bush_1_a",
            PropKind::KayKit_Bush_1_B => "kaykit_bush_1_b",
            PropKind::KayKit_Bush_2_A => "kaykit_bush_2_a",
            PropKind::KayKit_Bush_2_B => "kaykit_bush_2_b",
            PropKind::KayKit_Bush_3_A => "kaykit_bush_3_a",
            PropKind::KayKit_Bush_4_A => "kaykit_bush_4_a",

            // KayKit grass
            PropKind::KayKit_Grass_1_A => "kaykit_grass_1_a",
            PropKind::KayKit_Grass_1_B => "kaykit_grass_1_b",
            PropKind::KayKit_Grass_2_A => "kaykit_grass_2_a",
            PropKind::KayKit_Grass_2_B => "kaykit_grass_2_b",

            // Natureland
            PropKind::Nature_Pine_1 => "nature_pine_1",
            PropKind::Nature_Pine_2 => "nature_pine_2",
            PropKind::Nature_Pine_3 => "nature_pine_3",
            PropKind::Nature_Pine_4 => "nature_pine_4",
            PropKind::Nature_Pine_5 => "nature_pine_5",

            PropKind::Nature_TwistedTree_1 => "nature_twisted_tree_1",
            PropKind::Nature_TwistedTree_2 => "nature_twisted_tree_2",
            PropKind::Nature_TwistedTree_3 => "nature_twisted_tree_3",
            PropKind::Nature_TwistedTree_4 => "nature_twisted_tree_4",
            PropKind::Nature_TwistedTree_5 => "nature_twisted_tree_5",
            PropKind::Nature_CommonTree_1 => "nature_common_tree_1",
            PropKind::Nature_CommonTree_2 => "nature_common_tree_2",
            PropKind::Nature_CommonTree_3 => "nature_common_tree_3",

            PropKind::Nature_Bush_Common => "nature_bush_common",
            PropKind::Nature_Bush_Common_Flowers => "nature_bush_common_flowers",

            PropKind::Nature_Mushroom_Common => "nature_mushroom_common",
            PropKind::Nature_Mushroom_Laetiporus => "nature_mushroom_laetiporus",

            PropKind::Nature_Fern_1 => "nature_fern_1",
            PropKind::Nature_Plant_1 => "nature_plant_1",
            PropKind::Nature_Plant_1_Big => "nature_plant_1_big",
            PropKind::Nature_Plant_7 => "nature_plant_7",

            PropKind::Nature_Flower_3_Group => "nature_flower_3_group",
            PropKind::Nature_Flower_4_Group => "nature_flower_4_group",
            PropKind::Nature_Clover_1 => "nature_clover_1",
            PropKind::Nature_Clover_2 => "nature_clover_2",

            PropKind::Nature_Rock_Medium_1 => "nature_rock_medium_1",
            PropKind::Nature_Rock_Medium_2 => "nature_rock_medium_2",
            PropKind::Nature_Rock_Medium_3 => "nature_rock_medium_3",
            PropKind::Nature_Pebble_Round_1 => "nature_pebble_round_1",
            PropKind::Nature_Pebble_Round_2 => "nature_pebble_round_2",

            PropKind::Nature_Grass_Common_Short => "nature_grass_common_short",
            PropKind::Nature_Grass_Common_Tall => "nature_grass_common_tall",
            PropKind::Nature_Grass_Wispy_Short => "nature_grass_wispy_short",
            PropKind::Nature_Grass_Wispy_Tall => "nature_grass_wispy_tall",
        }
    }

    /// Asset scene path (used by the client to load visuals, and by the collider bake tool).
    pub const fn scene_path(&self) -> &'static str {
        match self {
            // KayKit rocks
            PropKind::KayKit_Rock_1_A => "Assetsfromassetpack/gltf/Rock_1_A_Color1.gltf#Scene0",
            PropKind::KayKit_Rock_1_B => "Assetsfromassetpack/gltf/Rock_1_B_Color1.gltf#Scene0",
            PropKind::KayKit_Rock_1_C => "Assetsfromassetpack/gltf/Rock_1_C_Color1.gltf#Scene0",
            PropKind::KayKit_Rock_2_A => "Assetsfromassetpack/gltf/Rock_2_A_Color1.gltf#Scene0",
            PropKind::KayKit_Rock_2_B => "Assetsfromassetpack/gltf/Rock_2_B_Color1.gltf#Scene0",
            PropKind::KayKit_Rock_3_A => "Assetsfromassetpack/gltf/Rock_3_A_Color1.gltf#Scene0",
            PropKind::KayKit_Rock_3_B => "Assetsfromassetpack/gltf/Rock_3_B_Color1.gltf#Scene0",
            PropKind::KayKit_Rock_3_C => "Assetsfromassetpack/gltf/Rock_3_C_Color1.gltf#Scene0",

            // KayKit dead trees
            PropKind::KayKit_Tree_Bare_1_A => "Assetsfromassetpack/gltf/Tree_Bare_1_A_Color1.gltf#Scene0",
            PropKind::KayKit_Tree_Bare_1_B => "Assetsfromassetpack/gltf/Tree_Bare_1_B_Color1.gltf#Scene0",
            PropKind::KayKit_Tree_Bare_2_A => "Assetsfromassetpack/gltf/Tree_Bare_2_A_Color1.gltf#Scene0",
            PropKind::KayKit_Tree_Bare_2_B => "Assetsfromassetpack/gltf/Tree_Bare_2_B_Color1.gltf#Scene0",

            // KayKit trees
            PropKind::KayKit_Tree_1_A => "Assetsfromassetpack/gltf/Tree_1_A_Color1.gltf#Scene0",
            PropKind::KayKit_Tree_1_B => "Assetsfromassetpack/gltf/Tree_1_B_Color1.gltf#Scene0",
            PropKind::KayKit_Tree_2_A => "Assetsfromassetpack/gltf/Tree_2_A_Color1.gltf#Scene0",
            PropKind::KayKit_Tree_2_B => "Assetsfromassetpack/gltf/Tree_2_B_Color1.gltf#Scene0",
            PropKind::KayKit_Tree_3_A => "Assetsfromassetpack/gltf/Tree_3_A_Color1.gltf#Scene0",
            PropKind::KayKit_Tree_4_A => "Assetsfromassetpack/gltf/Tree_4_A_Color1.gltf#Scene0",

            // KayKit bushes
            PropKind::KayKit_Bush_1_A => "Assetsfromassetpack/gltf/Bush_1_A_Color1.gltf#Scene0",
            PropKind::KayKit_Bush_1_B => "Assetsfromassetpack/gltf/Bush_1_B_Color1.gltf#Scene0",
            PropKind::KayKit_Bush_2_A => "Assetsfromassetpack/gltf/Bush_2_A_Color1.gltf#Scene0",
            PropKind::KayKit_Bush_2_B => "Assetsfromassetpack/gltf/Bush_2_B_Color1.gltf#Scene0",
            PropKind::KayKit_Bush_3_A => "Assetsfromassetpack/gltf/Bush_3_A_Color1.gltf#Scene0",
            PropKind::KayKit_Bush_4_A => "Assetsfromassetpack/gltf/Bush_4_A_Color1.gltf#Scene0",

            // KayKit grass
            PropKind::KayKit_Grass_1_A => "Assetsfromassetpack/gltf/Grass_1_A_Color1.gltf#Scene0",
            PropKind::KayKit_Grass_1_B => "Assetsfromassetpack/gltf/Grass_1_B_Color1.gltf#Scene0",
            PropKind::KayKit_Grass_2_A => "Assetsfromassetpack/gltf/Grass_2_A_Color1.gltf#Scene0",
            PropKind::KayKit_Grass_2_B => "Assetsfromassetpack/gltf/Grass_2_B_Color1.gltf#Scene0",

            // Natureland
            PropKind::Nature_Pine_1 => "StylizedNature/glTF/Pine_1.gltf#Scene0",
            PropKind::Nature_Pine_2 => "StylizedNature/glTF/Pine_2.gltf#Scene0",
            PropKind::Nature_Pine_3 => "StylizedNature/glTF/Pine_3.gltf#Scene0",
            PropKind::Nature_Pine_4 => "StylizedNature/glTF/Pine_4.gltf#Scene0",
            PropKind::Nature_Pine_5 => "StylizedNature/glTF/Pine_5.gltf#Scene0",

            PropKind::Nature_TwistedTree_1 => "StylizedNature/glTF/TwistedTree_1.gltf#Scene0",
            PropKind::Nature_TwistedTree_2 => "StylizedNature/glTF/TwistedTree_2.gltf#Scene0",
            PropKind::Nature_TwistedTree_3 => "StylizedNature/glTF/TwistedTree_3.gltf#Scene0",
            PropKind::Nature_TwistedTree_4 => "StylizedNature/glTF/TwistedTree_4.gltf#Scene0",
            PropKind::Nature_TwistedTree_5 => "StylizedNature/glTF/TwistedTree_5.gltf#Scene0",
            PropKind::Nature_CommonTree_1 => "StylizedNature/glTF/CommonTree_1.gltf#Scene0",
            PropKind::Nature_CommonTree_2 => "StylizedNature/glTF/CommonTree_2.gltf#Scene0",
            PropKind::Nature_CommonTree_3 => "StylizedNature/glTF/CommonTree_3.gltf#Scene0",

            PropKind::Nature_Bush_Common => "StylizedNature/glTF/Bush_Common.gltf#Scene0",
            PropKind::Nature_Bush_Common_Flowers => "StylizedNature/glTF/Bush_Common_Flowers.gltf#Scene0",

            PropKind::Nature_Mushroom_Common => "StylizedNature/glTF/Mushroom_Common.gltf#Scene0",
            PropKind::Nature_Mushroom_Laetiporus => "StylizedNature/glTF/Mushroom_Laetiporus.gltf#Scene0",

            PropKind::Nature_Fern_1 => "StylizedNature/glTF/Fern_1.gltf#Scene0",
            PropKind::Nature_Plant_1 => "StylizedNature/glTF/Plant_1.gltf#Scene0",
            PropKind::Nature_Plant_1_Big => "StylizedNature/glTF/Plant_1_Big.gltf#Scene0",
            PropKind::Nature_Plant_7 => "StylizedNature/glTF/Plant_7.gltf#Scene0",

            PropKind::Nature_Flower_3_Group => "StylizedNature/glTF/Flower_3_Group.gltf#Scene0",
            PropKind::Nature_Flower_4_Group => "StylizedNature/glTF/Flower_4_Group.gltf#Scene0",
            PropKind::Nature_Clover_1 => "StylizedNature/glTF/Clover_1.gltf#Scene0",
            PropKind::Nature_Clover_2 => "StylizedNature/glTF/Clover_2.gltf#Scene0",

            PropKind::Nature_Rock_Medium_1 => "StylizedNature/glTF/Rock_Medium_1.gltf#Scene0",
            PropKind::Nature_Rock_Medium_2 => "StylizedNature/glTF/Rock_Medium_2.gltf#Scene0",
            PropKind::Nature_Rock_Medium_3 => "StylizedNature/glTF/Rock_Medium_3.gltf#Scene0",
            PropKind::Nature_Pebble_Round_1 => "StylizedNature/glTF/Pebble_Round_1.gltf#Scene0",
            PropKind::Nature_Pebble_Round_2 => "StylizedNature/glTF/Pebble_Round_2.gltf#Scene0",

            PropKind::Nature_Grass_Common_Short => "StylizedNature/glTF/Grass_Common_Short.gltf#Scene0",
            PropKind::Nature_Grass_Common_Tall => "StylizedNature/glTF/Grass_Common_Tall.gltf#Scene0",
            PropKind::Nature_Grass_Wispy_Short => "StylizedNature/glTF/Grass_Wispy_Short.gltf#Scene0",
            PropKind::Nature_Grass_Wispy_Tall => "StylizedNature/glTF/Grass_Wispy_Tall.gltf#Scene0",
        }
    }
}

/// All prop kinds currently used in the world (client loads these at startup).
pub const ALL_PROP_KINDS: &[PropKind] = &[
    // KayKit rocks
    PropKind::KayKit_Rock_1_A,
    PropKind::KayKit_Rock_1_B,
    PropKind::KayKit_Rock_1_C,
    PropKind::KayKit_Rock_2_A,
    PropKind::KayKit_Rock_2_B,
    PropKind::KayKit_Rock_3_A,
    PropKind::KayKit_Rock_3_B,
    PropKind::KayKit_Rock_3_C,
    // KayKit dead trees
    PropKind::KayKit_Tree_Bare_1_A,
    PropKind::KayKit_Tree_Bare_1_B,
    PropKind::KayKit_Tree_Bare_2_A,
    PropKind::KayKit_Tree_Bare_2_B,
    // KayKit trees
    PropKind::KayKit_Tree_1_A,
    PropKind::KayKit_Tree_1_B,
    PropKind::KayKit_Tree_2_A,
    PropKind::KayKit_Tree_2_B,
    PropKind::KayKit_Tree_3_A,
    PropKind::KayKit_Tree_4_A,
    // KayKit bushes
    PropKind::KayKit_Bush_1_A,
    PropKind::KayKit_Bush_1_B,
    PropKind::KayKit_Bush_2_A,
    PropKind::KayKit_Bush_2_B,
    PropKind::KayKit_Bush_3_A,
    PropKind::KayKit_Bush_4_A,
    // KayKit grass
    PropKind::KayKit_Grass_1_A,
    PropKind::KayKit_Grass_1_B,
    PropKind::KayKit_Grass_2_A,
    PropKind::KayKit_Grass_2_B,
    // Natureland pines
    PropKind::Nature_Pine_1,
    PropKind::Nature_Pine_2,
    PropKind::Nature_Pine_3,
    PropKind::Nature_Pine_4,
    PropKind::Nature_Pine_5,
    // Natureland trees
    PropKind::Nature_TwistedTree_1,
    PropKind::Nature_TwistedTree_2,
    PropKind::Nature_TwistedTree_3,
    PropKind::Nature_TwistedTree_4,
    PropKind::Nature_TwistedTree_5,
    PropKind::Nature_CommonTree_1,
    PropKind::Nature_CommonTree_2,
    PropKind::Nature_CommonTree_3,
    // Natureland bushes
    PropKind::Nature_Bush_Common,
    PropKind::Nature_Bush_Common_Flowers,
    // Natureland mushrooms
    PropKind::Nature_Mushroom_Common,
    PropKind::Nature_Mushroom_Laetiporus,
    // Natureland ferns/plants
    PropKind::Nature_Fern_1,
    PropKind::Nature_Plant_1,
    PropKind::Nature_Plant_1_Big,
    PropKind::Nature_Plant_7,
    // Natureland flowers
    PropKind::Nature_Flower_3_Group,
    PropKind::Nature_Flower_4_Group,
    PropKind::Nature_Clover_1,
    PropKind::Nature_Clover_2,
    // Natureland rocks
    PropKind::Nature_Rock_Medium_1,
    PropKind::Nature_Rock_Medium_2,
    PropKind::Nature_Rock_Medium_3,
    PropKind::Nature_Pebble_Round_1,
    PropKind::Nature_Pebble_Round_2,
    // Natureland grass
    PropKind::Nature_Grass_Common_Short,
    PropKind::Nature_Grass_Common_Tall,
    PropKind::Nature_Grass_Wispy_Short,
    PropKind::Nature_Grass_Wispy_Tall,
];

const DESERT_ROCKS: &[PropKind] = &[
    PropKind::KayKit_Rock_1_A,
    PropKind::KayKit_Rock_1_B,
    PropKind::KayKit_Rock_1_C,
    PropKind::KayKit_Rock_2_A,
    PropKind::KayKit_Rock_2_B,
    PropKind::KayKit_Rock_3_A,
    PropKind::KayKit_Rock_3_B,
    PropKind::KayKit_Rock_3_C,
];

const DESERT_DEAD_TREES: &[PropKind] = &[
    PropKind::KayKit_Tree_Bare_1_A,
    PropKind::KayKit_Tree_Bare_1_B,
    PropKind::KayKit_Tree_Bare_2_A,
    PropKind::KayKit_Tree_Bare_2_B,
];

const GRASSLAND_TREES: &[PropKind] = &[
    PropKind::KayKit_Tree_1_A,
    PropKind::KayKit_Tree_1_B,
    PropKind::KayKit_Tree_2_A,
    PropKind::KayKit_Tree_2_B,
    PropKind::KayKit_Tree_3_A,
    PropKind::KayKit_Tree_4_A,
];

const GRASSLAND_BUSHES: &[PropKind] = &[
    PropKind::KayKit_Bush_1_A,
    PropKind::KayKit_Bush_1_B,
    PropKind::KayKit_Bush_2_A,
    PropKind::KayKit_Bush_2_B,
    PropKind::KayKit_Bush_3_A,
    PropKind::KayKit_Bush_4_A,
];

const GRASSLAND_GRASS: &[PropKind] = &[
    PropKind::KayKit_Grass_1_A,
    PropKind::KayKit_Grass_1_B,
    PropKind::KayKit_Grass_2_A,
    PropKind::KayKit_Grass_2_B,
];

const NATURE_PINES: &[PropKind] = &[
    PropKind::Nature_Pine_1,
    PropKind::Nature_Pine_2,
    PropKind::Nature_Pine_3,
    PropKind::Nature_Pine_4,
    PropKind::Nature_Pine_5,
];

const NATURE_TREES: &[PropKind] = &[
    PropKind::Nature_TwistedTree_1,
    PropKind::Nature_TwistedTree_2,
    PropKind::Nature_TwistedTree_3,
    PropKind::Nature_TwistedTree_4,
    PropKind::Nature_TwistedTree_5,
    PropKind::Nature_CommonTree_1,
    PropKind::Nature_CommonTree_2,
    PropKind::Nature_CommonTree_3,
];

const NATURE_BUSHES: &[PropKind] = &[
    PropKind::Nature_Bush_Common,
    PropKind::Nature_Bush_Common_Flowers,
];

const NATURE_MUSHROOMS: &[PropKind] = &[
    PropKind::Nature_Mushroom_Common,
    PropKind::Nature_Mushroom_Laetiporus,
];

const NATURE_FERNS: &[PropKind] = &[
    PropKind::Nature_Fern_1,
    PropKind::Nature_Plant_1,
    PropKind::Nature_Plant_1_Big,
    PropKind::Nature_Plant_7,
];

const NATURE_FLOWERS: &[PropKind] = &[
    PropKind::Nature_Flower_3_Group,
    PropKind::Nature_Flower_4_Group,
    PropKind::Nature_Clover_1,
    PropKind::Nature_Clover_2,
];

const NATURE_ROCKS: &[PropKind] = &[
    PropKind::Nature_Rock_Medium_1,
    PropKind::Nature_Rock_Medium_2,
    PropKind::Nature_Rock_Medium_3,
    PropKind::Nature_Pebble_Round_1,
    PropKind::Nature_Pebble_Round_2,
];

const NATURE_GRASS: &[PropKind] = &[
    PropKind::Nature_Grass_Common_Short,
    PropKind::Nature_Grass_Common_Tall,
    PropKind::Nature_Grass_Wispy_Short,
    PropKind::Nature_Grass_Wispy_Tall,
];

/// A single prop spawn (deterministic from world seed + chunk coord).
#[derive(Debug, Clone, Copy)]
pub struct PropSpawn {
    pub kind: PropKind,
    pub chunk: ChunkCoord,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: f32,
    pub render_tuning: PropRenderTuning,
}

/// Deterministically generate all prop spawns for a given chunk.
///
/// This mirrors the previous logic in `client/src/props.rs`, but is now shared.
pub fn generate_chunk_prop_spawns(terrain: &TerrainGenerator, chunk: ChunkCoord) -> Vec<PropSpawn> {
    let mut out = Vec::new();

    // Deterministic noise for prop placement.
    let placement_noise = Perlin::new(WORLD_SEED.wrapping_add(5000));
    let density_noise = Perlin::new(WORLD_SEED.wrapping_add(6000));
    let variety_noise = Perlin::new(WORLD_SEED.wrapping_add(7000));

    let chunk_origin = chunk.world_pos();
    let center_x = chunk_origin.x + CHUNK_SIZE / 2.0;
    let center_z = chunk_origin.z + CHUNK_SIZE / 2.0;
    let chunk_biome = terrain.get_biome(center_x, center_z);

    // Natureland uses wider spacing due to more detailed models.
    let grid_spacing = match chunk_biome {
        Biome::Natureland => 12.0,
        _ => 8.0,
    };
    let steps = (CHUNK_SIZE / grid_spacing) as i32;

    for gz in 0..steps {
        for gx in 0..steps {
            let base_x = chunk_origin.x + gx as f32 * grid_spacing + grid_spacing * 0.5;
            let base_z = chunk_origin.z + gz as f32 * grid_spacing + grid_spacing * 0.5;

            // Deterministic jitter
            let jitter_x = placement_noise.get([base_x as f64 * 0.1, base_z as f64 * 0.1]) as f32
                * grid_spacing
                * 0.4;
            let jitter_z = placement_noise.get([base_z as f64 * 0.1, base_x as f64 * 0.1]) as f32
                * grid_spacing
                * 0.4;

            let world_x = base_x + jitter_x;
            let world_z = base_z + jitter_z;
            let world_y = terrain.get_height(world_x, world_z);

            let biome = terrain.get_biome(world_x, world_z);
            let density = density_noise.get([world_x as f64 * 0.05, world_z as f64 * 0.05]) as f32;
            let variety = variety_noise.get([world_x as f64 * 0.3, world_z as f64 * 0.3]) as f32;

            let position = Vec3::new(world_x, world_y, world_z);

            let abs_v = variety.abs();
            let rot = Quat::from_rotation_y(variety * std::f32::consts::TAU);

            match biome {
                Biome::Desert => {
                    // Skip props inside settlement zones (villages have buildings, not rocks)
                    if terrain.is_in_settlement(world_x, world_z) {
                        continue;
                    }
                    
                    // Rocks: ~30% chance at each grid point
                    if density > 0.2 {
                        let idx = ((abs_v * 100.0) as usize) % DESERT_ROCKS.len();
                        let scale = 0.8 + abs_v * 0.6; // 0.8-1.4
                        out.push(PropSpawn {
                            kind: DESERT_ROCKS[idx],
                            chunk,
                            position,
                            rotation: rot,
                            scale,
                            render_tuning: PropRenderTuning { casts_shadows: true, visible_end_distance: None },
                        });
                    }
                    // Dead trees: ~5% chance (very rare)
                    else if density > 0.1 && density < 0.15 {
                        let idx = ((abs_v * 100.0) as usize) % DESERT_DEAD_TREES.len();
                        let scale = 1.5 + abs_v * 0.5;
                        out.push(PropSpawn {
                            kind: DESERT_DEAD_TREES[idx],
                            chunk,
                            position,
                            rotation: rot,
                            scale,
                            render_tuning: PropRenderTuning { casts_shadows: true, visible_end_distance: None },
                        });
                    }
                }
                Biome::Grasslands => {
                    // Trees: ~15% chance
                    if density > 0.35 {
                        let idx = ((abs_v * 100.0) as usize) % GRASSLAND_TREES.len();
                        let scale = 1.2 + abs_v * 0.8; // 1.2-2.0
                        out.push(PropSpawn {
                            kind: GRASSLAND_TREES[idx],
                            chunk,
                            position,
                            rotation: rot,
                            scale,
                            render_tuning: PropRenderTuning { casts_shadows: true, visible_end_distance: None },
                        });
                    }
                    // Bushes: ~20% chance
                    else if density > 0.15 && density < 0.35 {
                        let idx = ((abs_v * 100.0) as usize) % GRASSLAND_BUSHES.len();
                        let scale = 0.8 + abs_v * 0.4;
                        out.push(PropSpawn {
                            kind: GRASSLAND_BUSHES[idx],
                            chunk,
                            position,
                            rotation: rot,
                            scale,
                            render_tuning: PropRenderTuning { casts_shadows: false, visible_end_distance: Some(90.0) },
                        });
                    }
                    // Grass: ~25% chance
                    else if density > -0.1 && density < 0.15 {
                        let idx = ((abs_v * 100.0) as usize) % GRASSLAND_GRASS.len();
                        let scale = 0.6 + abs_v * 0.3;
                        out.push(PropSpawn {
                            kind: GRASSLAND_GRASS[idx],
                            chunk,
                            position,
                            rotation: rot,
                            scale,
                            render_tuning: PropRenderTuning { casts_shadows: false, visible_end_distance: Some(80.0) },
                        });
                    }
                }
                Biome::Natureland => {
                    // Pine trees: ~12% chance
                    if density > 0.4 {
                        let idx = ((abs_v * 100.0) as usize) % NATURE_PINES.len();
                        let scale = 1.0 + abs_v * 0.5;
                        out.push(PropSpawn {
                            kind: NATURE_PINES[idx],
                            chunk,
                            position,
                            rotation: rot,
                            scale,
                            render_tuning: PropRenderTuning { casts_shadows: true, visible_end_distance: None },
                        });
                    }
                    // Twisted/common trees: ~10% chance
                    else if density > 0.3 && density < 0.4 {
                        let idx = ((abs_v * 100.0) as usize) % NATURE_TREES.len();
                        let scale = 0.9 + abs_v * 0.4;
                        out.push(PropSpawn {
                            kind: NATURE_TREES[idx],
                            chunk,
                            position,
                            rotation: rot,
                            scale,
                            render_tuning: PropRenderTuning { casts_shadows: true, visible_end_distance: None },
                        });
                    }
                    // Bushes: ~12% chance
                    else if density > 0.18 && density < 0.3 {
                        let idx = ((abs_v * 100.0) as usize) % NATURE_BUSHES.len();
                        let scale = 0.7 + abs_v * 0.4;
                        out.push(PropSpawn {
                            kind: NATURE_BUSHES[idx],
                            chunk,
                            position,
                            rotation: rot,
                            scale,
                            render_tuning: PropRenderTuning { casts_shadows: false, visible_end_distance: Some(90.0) },
                        });
                    }
                    // Ferns and plants: ~15% chance
                    else if density > 0.03 && density < 0.18 {
                        let idx = ((abs_v * 100.0) as usize) % NATURE_FERNS.len();
                        let scale = 0.6 + abs_v * 0.3;
                        out.push(PropSpawn {
                            kind: NATURE_FERNS[idx],
                            chunk,
                            position,
                            rotation: rot,
                            scale,
                            render_tuning: PropRenderTuning { casts_shadows: false, visible_end_distance: Some(70.0) },
                        });
                    }
                    // Grass: ~15% chance
                    else if density > -0.12 && density < 0.03 {
                        let idx = ((abs_v * 100.0) as usize) % NATURE_GRASS.len();
                        let scale = 0.5 + abs_v * 0.3;
                        out.push(PropSpawn {
                            kind: NATURE_GRASS[idx],
                            chunk,
                            position,
                            rotation: rot,
                            scale,
                            render_tuning: PropRenderTuning { casts_shadows: false, visible_end_distance: Some(75.0) },
                        });
                    }
                    // Flowers: ~8% chance
                    else if density > -0.2 && density < -0.12 {
                        let idx = ((abs_v * 100.0) as usize) % NATURE_FLOWERS.len();
                        let scale = 0.5 + abs_v * 0.25;
                        out.push(PropSpawn {
                            kind: NATURE_FLOWERS[idx],
                            chunk,
                            position,
                            rotation: rot,
                            scale,
                            render_tuning: PropRenderTuning { casts_shadows: false, visible_end_distance: Some(55.0) },
                        });
                    }
                    // Mushrooms: ~5% chance
                    else if density > -0.25 && density < -0.2 {
                        let idx = ((abs_v * 100.0) as usize) % NATURE_MUSHROOMS.len();
                        let scale = 0.4 + abs_v * 0.3;
                        out.push(PropSpawn {
                            kind: NATURE_MUSHROOMS[idx],
                            chunk,
                            position,
                            rotation: rot,
                            scale,
                            render_tuning: PropRenderTuning { casts_shadows: false, visible_end_distance: Some(30.0) },
                        });
                    }
                    // Rocks: ~5% chance
                    else if density > -0.3 && density < -0.25 {
                        let idx = ((abs_v * 100.0) as usize) % NATURE_ROCKS.len();
                        let scale = 0.6 + abs_v * 0.4;
                        out.push(PropSpawn {
                            kind: NATURE_ROCKS[idx],
                            chunk,
                            position,
                            rotation: rot,
                            scale,
                            render_tuning: PropRenderTuning { casts_shadows: true, visible_end_distance: Some(180.0) },
                        });
                    }
                }
            }
        }
    }

    out
}

