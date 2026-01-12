# FistForce

A multiplayer 3D sandbox shooter built with **Rust** and **Bevy 0.17**.

## Features

- 🏜️ Multiple biomes (Desert, Grasslands, Natureland) with procedural prop placement
- 🎯 Server-authoritative shooting with realistic bullet ballistics (drop, travel time)
- 🤖 NPCs with pathfinding AI and hit detection (headshots, body zones)
- 🌅 Dynamic day/night cycle with atmospheric scattering
- 🚗 Driveable vehicles (motorbike)
- 🎮 Client-side prediction with server reconciliation
- 🌲 Baked convex-hull colliders for environment props (trees, rocks)

---

## Workspace Layout

| Crate | Description |
|-------|-------------|
| `client/` | Bevy app with rendering, input, UI, terrain/prop streaming |
| `server/` | Headless authoritative server (physics, AI, hit detection) |
| `shared/` | Deterministic terrain/props, protocol, components, ballistics |
| `tools/collider_baker/` | Offline tool to bake convex-hull colliders from GLTF meshes |

Assets live in `client/assets/` (models, audio, `colliders.bin`).

---

## Architecture Overview

### Networking (Lightyear)

- **Server-authoritative**: The server owns all gameplay state (positions, health, bullets).
- **Client-side prediction**: The client predicts local player movement; server corrects if needed.
- **Replication**: Components marked with `Replicate` are automatically synced to clients.
- **Messages**: `PlayerInput`, `ShootRequest`, `HitConfirm`, `BulletImpact`, etc.

### Terrain & Props

- **Deterministic generation**: Both client and server generate identical terrain/prop layouts from the same seed (`shared/src/terrain.rs`, `shared/src/props.rs`).
- **Chunk streaming**: Client loads/unloads terrain meshes and props based on camera position.
- **Biomes**: Desert, Grasslands, Natureland — each with different props and surface friction.

### Collisions

- **Ground**: Heightfield lookup (`terrain.generator.get_height(x, z)`).
- **Static props**: Baked convex-hull colliders loaded at server startup; spatial-hash streaming around players.
- **Entities**: Capsule (player/NPC) and OBB (vehicle) vs convex-hull resolution.

### Weapons & Combat

- **Ballistics**: Bullets are physical projectiles with velocity, gravity, drag.
- **Hit detection**: Server raycasts against NPC/player hitboxes (head, chest, limbs).
- **Recoil**: Accumulative recoil for rapid fire; reduced when ADS.

---

## Quick Start

```bash
# Build and run (starts server in background, then client)
./run.sh
```

Or manually:

```bash
# Terminal 1 — server
cargo run -p server --release

# Terminal 2 — client
cargo run -p client --release
```

---

## Collider Baker

Environment props (trees, rocks) use **pre-baked convex-hull colliders** so the server doesn't need to load meshes at runtime.

### When to run

- After adding new collidable props to `client/assets/`
- After editing `client/assets/colliders_manifest.ron`
- After changing a prop's mesh file

### How to run

```bash
cargo run -p collider_baker --release
```

This reads `colliders_manifest.ron`, loads each GLTF, computes a convex hull (with optional trunk-slice for trees), and writes `colliders.bin`.

### Manifest format (`colliders_manifest.ron`)

```ron
(
    entries: [
        ( kind: "KayKitTree1A", gltf_path: "Assetsfromassetpack/gltf/tree_1a.gltf", mode: ConvexHull, vertex_filter: LowerYPercent(0.35) ),
        ( kind: "KayKitRock1A", gltf_path: "Assetsfromassetpack/gltf/rock_1a.gltf", mode: ConvexHull, vertex_filter: All ),
        // ...
    ]
)
```

- `vertex_filter: LowerYPercent(0.35)` → only use the bottom 35% of vertices (avoids giant canopy colliders on trees).
- `vertex_filter: All` → use all vertices (rocks, buildings).

---

## Controls

| Key | Action |
|-----|--------|
| WASD | Move |
| Space | Jump |
| Mouse | Look |
| LMB | Shoot |
| RMB | Toggle ADS (Aim Down Sights) |
| R | Reload |
| E | Enter/exit vehicle |
| 1-4 | Switch weapon |
| F3 | Toggle debug overlay |
| Esc | Release cursor / Pause menu |

---

## Debug Overlay (F3)

When enabled, the overlay shows:

- **FPS** (color-coded: green ≥55, yellow ≥30, red <30)
- **Entity count**
- **Loaded terrain chunks**
- **Props** (total and collidable)
- **Collider chunks** (server streaming radius)
- **Render CPU times** (top 5 passes)

Gizmos are drawn for:

- Bullet trajectories (green lines)
- NPC hitboxes (body capsule + head sphere)
- Collidable prop colliders (cyan cylinders)

---

## Tech Stack

| Crate | Purpose |
|-------|---------|
| [Bevy 0.17](https://bevyengine.org/) | Game engine (ECS, rendering, audio) |
| [Lightyear 0.25](https://github.com/cBournhonesque/lightyear) | Networking (replication, prediction) |
| [bevy_rapier3d](https://github.com/dimforge/bevy_rapier) | Convex-hull computation (bake tool only) |
| [noise](https://docs.rs/noise) | Perlin noise for terrain & prop placement |
| [ron](https://docs.rs/ron) | Manifest file format |
| [bincode](https://docs.rs/bincode) | Baked collider serialization |

---

## License

Assets from [KayKit](https://kaylousberg.itch.io/) and [Stylized Nature MegaKit](https://quaternius.com/) — see their respective license files in `client/assets/`.
