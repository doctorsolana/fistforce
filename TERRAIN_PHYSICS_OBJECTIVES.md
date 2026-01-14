### Terrain + Physics Objectives (Worklist)

This document captures the full set of improvements we discussed for terrain deformation + physics consistency + building integration. Intended to be worked through one-by-one.

---

### Guiding principles
- **Single source of truth for ground**: all gameplay height/normal queries go through `WorldTerrain` (not `TerrainGenerator` directly).
- **Consistency**: server physics, client rendering, bullets, props, and building validation must agree on the same terrain function.
- **Scalability**: terrain edits must scale to many buildings without O(N_buildings) checks per query.
- **Valheim-style building pads**: props/colliders inside build areas are cleared.

---

### Current confirmed design decisions
- **Props/colliders in build areas**: clear them (despawn/skip spawn inside build zones).
- **Terrain deformation representation**: per-chunk **height-delta grids** (vertex-aligned), not a list of zones/points.

---

### Worklist (recommended order)

#### 1) Terrain delta core (shared) [FOUNDATION]
- **Goal**: represent edits as per-chunk delta grids and make `WorldTerrain::get_height()` return `procedural + delta`.
- **Changes**
  - Add chunk delta store to `WorldTerrain` (e.g. `HashMap<ChunkCoord, TerrainDeltaData>`).
  - Implement bilinear sampling of delta by (x,z).
  - Make `WorldTerrain::get_normal()` sample unified height (so normals reflect edits).
  - Add `apply_flatten_rect(center, half_extents, rotation_y, blend_width)` that:
    - determines affected chunks
    - edits vertex-aligned delta grids
    - blends edges smoothly (smoothstep)
    - composes with existing edits (don't "forget" prior edits)
- **Acceptance criteria**
  - Modified height queries work everywhere that calls `terrain.get_height()`.
  - Flattening works over slopes (raise + lower) without floating/sinking.
  - No reliance on scanning build zones during gameplay height queries.

#### 2) Network replication: per-chunk delta components [SCALABILITY]
- **Goal**: replicate edits chunk-by-chunk instead of "whole-world modification blobs".
- **Changes**
  - Add replicated component like `TerrainDeltaChunk { coord, deltas_cm: Vec<i16>, version }`.
  - Quantize deltas to cm in `i16` to keep bandwidth small.
  - Register in `shared/src/protocol.rs`.
  - Deprecate/remove old `TerrainModifications` replication path.
- **Acceptance criteria**
  - Server can publish delta chunks; clients receive them reliably.
  - A late-joining client receives enough data to reconstruct modified terrain.

#### 3) Server: apply edits + publish delta chunks + mark dirty [AUTHORITY]
- **Goal**: building placement authoritatively edits terrain and pushes per-chunk deltas to clients.
- **Changes**
  - On `PlaceBuildingRequest`, server:
    - validates cost and placement
    - computes target height
    - calls `terrain.apply_flatten_rect(...)`
    - upserts replicated `TerrainDeltaChunk` entities for affected chunks
  - Maintain mapping `ChunkCoord -> Entity` for delta chunk entities (so updates overwrite).
- **Acceptance criteria**
  - Placing a building updates terrain for all clients.
  - Reconnecting / new client still sees old edits.

#### 4) Client: ingest delta chunks + regenerate terrain meshes [VISUALS]
- **Goal**: client merges replicated delta chunks into `WorldTerrain` and regenerates affected chunk meshes.
- **Changes**
  - Query replicated `TerrainDeltaChunk` and update local `WorldTerrain` delta store.
  - Mark dirty chunks (and **neighbors**, to reduce normal seams).
  - Despawn + respawn dirty `TerrainChunk` meshes via existing streaming pipeline.
- **Acceptance criteria**
  - Terrain visibly changes for all clients after placement.
  - No "stale mesh" after edits.

#### 5) Build zones: clear props + clear prop colliders [VALHEIM PAD]
- **Goal**: props/trees/rocks are removed (and server colliders removed) inside building footprint + blend zone.
- **Changes**
  - Add shared helper in `shared/src/building.rs`:
    - `point_in_rotated_rect(point_xz, center_xz, half_extents, rotation_y) -> bool`
  - Client props:
    - skip spawning props inside build zones
    - resample prop Y using `terrain.get_height()` so props outside zones sit on modified terrain
    - when a building is placed: mark affected prop chunks dirty and respawn
  - Server prop colliders:
    - filter deterministic prop spawns by build zones
    - unload/reload affected collider chunks after placement
- **Acceptance criteria**
  - No trees/rocks inside building pads.
  - No invisible prop colliders inside pads.
  - Props outside pads sit on the modified terrain correctly.

#### 6) Physics hardening: real grounded + remove hack [GAME FEEL]
- **Goal**: remove velocity-based grounded hack and base jumping on real support contacts.
- **Changes**
  - Add `PlayerGrounded` (and optionally `NpcGrounded`) component.
  - Compute grounded in server collision resolution based on:
    - terrain proximity OR
    - contact normal from props/structures with upward-ish normal.
  - Update `shared/src/physics.rs`:
    - remove `not_falling_fast` grounded logic
    - allow jump based on grounded state from previous tick
    - optionally add small "coyote time"
- **Acceptance criteria**
  - No accidental air-jumps.
  - Jumping works reliably on terrain and on top of props/structures.

#### 7) Step-up implementation (capsule vs static/structures) [FEEL + ACCESSIBILITY]
- **Goal**: small ledges/rocks don't snag; stairs feel good.
- **Changes**
  - Implement real `STEP_UP_HEIGHT` behavior in server collision:
    - if blocked by near-vertical obstacle, attempt upward offset <= step height, then re-resolve
- **Acceptance criteria**
  - Walking over small rocks/edges feels smooth.
  - No "sticky corners" as often.

#### 8) Build preview matches real flatten math [UX]
- **Goal**: preview shows what the server will actually do.
- **Changes**
  - Replace current "flat plane" preview with a low-res mesh that samples predicted post-flatten height using the same smoothstep math as the server.
- **Acceptance criteria**
  - Preview reflects both raised and lowered terrain, not just a plane.

#### 9) Cleanup + docs [SAFETY]
- **Goal**: eliminate old paths and reduce future regressions.
- **Changes**
  - Remove legacy terrain mod APIs (`TerrainModifications`, `get_height_with_mods`, etc.) after delta system is in.
  - Remove/avoid direct `terrain.generator.get_height()` call sites (except biome/noise decisions).
  - Update README collision docs to reference `WorldTerrain::get_height()`.
- **Acceptance criteria**
  - No duplicate terrain APIs that can accidentally be used.
  - The repo clearly communicates the "one true way" to query terrain.

---

### Notes / gotchas to watch
- **Chunk seams**: when a delta chunk changes, regen neighbor chunks too for normals.
- **Bandwidth**: prefer per-chunk diffs or compressed/quantized arrays over re-sending full world edits.
- **Determinism**: build-zone filtering must use replicated building state (not local guesses).
- **Order of ops on server tick**: keep "apply edits" before "collider streaming / collisions" so physics uses updated terrain immediately.
