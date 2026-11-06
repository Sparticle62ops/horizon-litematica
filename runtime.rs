// Litematica for Horizon: the schematic ghost overlay's render glue.
//
// A Rust port of Litematica (Copyright maruohon, LGPL-3.0,
// https://github.com/maruohon/litematica). The overlay's pure logic lives in
// the bundle crate litematica_mod (src/overlay.rs and friends). This file is
// the Horizon glue: it holds the loaded overlay in a static, handles the
// /litematica commands, and the world-primitives hook draws it as translucent
// filled boxes with outlines (the compromise documented while Horizon has no
// block-model world primitive; an issue proposing that primitive tracks on
// horizon-rust/horizon).
//
// This file is the module the bundle adds to the game.

use std::sync::Mutex;

use litematica_mod::overlay::{color_for, GhostBlock};
use litematica_mod::commands::{load_schematic_file, save_loaded_schematic, parse_command, Command, SelectSub};
use litematica_mod::printer::{PrintStep, print_order_ghosts};
use horizon_client::command::{Reader, block};
use horizon_world::registry::BlockStateId;

/// The mod's id.
pub const ID: &str = "litematica";

/// The render distance cap for ghost boxes, in blocks (schematic previews
/// can be big; beyond this the boxes skip).
pub const RENDER_DISTANCE: f64 = 128.0;

/// The outline width at the camera.
pub const OUTLINE_WIDTH: f32 = 1.5;

/// The outline color drawn over every ghost fill (white).
pub const OUTLINE_COLOR: u32 = 0xC8FF_FFFF;

static CURRENT: Mutex<Vec<GhostBlock>> = Mutex::new(Vec::new());

/// Install one computed overlay as the active one (replacing any previous).
pub fn load(ghosts: Vec<GhostBlock>) {
    if let Ok(mut v) = CURRENT.lock() {
        *v = ghosts;
    }
}

/// Drop the active overlay.
pub fn clear() {
    if let Ok(mut v) = CURRENT.lock() {
        v.clear();
    }
}

/// The number of ghosts currently active (for a status readout).
pub fn len() -> usize {
    CURRENT.lock().map(|v| v.len()).unwrap_or(0)
}

/// The squared distance between two points.
pub fn dist_sqr(a: [f64; 3], b: [f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

impl crate::game::Game<'_> {
    /// The world-primitives hook: one translucent filled box plus outline per
    /// active ghost block inside the render distance, through walls.
    pub fn litematica_draw(
        &self,
        camera: [f64; 3],
        primitives: &mut crate::world_primitives::WorldPrimitives,
    ) {
        if !self.native_on(ID) {
            return;
        }
        let ghosts = CURRENT.lock().map(|v| v.clone()).unwrap_or_default();
        if ghosts.is_empty() {
            return;
        }
        for ghost in ghosts {
            let (x, y, z) = (ghost.world.0 as f64, ghost.world.1 as f64, ghost.world.2 as f64);
            let centre = [x + 0.5, y + 0.5, z + 0.5];
            if dist_sqr(camera, centre) > RENDER_DISTANCE * RENDER_DISTANCE {
                continue;
            }
            let min = [x, y, z];
            let max = [x + 1.0, y + 1.0, z + 1.0];
            let fill = color_for(ghost.kind);
            primitives
                .filled_box(min, max, fill)
                .through_walls();
            primitives
                .box_outline(min, max, OUTLINE_COLOR, OUTLINE_WIDTH)
                .through_walls();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use self::OverlayKind::*;

    #[test]
    fn load_replaces_and_clear_empties() {
        load(vec![GhostBlock {
            world: [0, 0, 0],
            kind: OverlayKind::Missing,
            state_key: "minecraft:stone".into(),
        }]);
        assert_eq!(len(), 1);
        load(vec![]);
        assert_eq!(len(), 0);
        load(vec![GhostBlock {
            world: [1, 1, 1],
            kind: OverlayKind::Wrong,
            state_key: "minecraft:dirt".into(),
        }]);
        clear();
        assert_eq!(len(), 0);
    }

    #[test]
    fn distance_culling_math() {
        assert!(dist_sqr([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]) < RENDER_DISTANCE * RENDER_DISTANCE);
        assert!(
            dist_sqr([0.0, 0.0, 0.0], [200.0, 0.0, 0.0]) > RENDER_DISTANCE * RENDER_DISTANCE
        );
    }
}

/// The origin the last loaded schematic was placed at (bare verify re-checks it).
static LAST_LOAD_ORIGIN: Mutex<[i32; 3]> = Mutex::new([0, 0, 0]);

/// A world-reading command waiting for the next level tick (the command hook
/// has no Game access, so verify and save-area run there, like the printer).
enum PendingWorldCmd {
    SaveArea { origin: [i32; 3], size: [i32; 3], path: String },
    ClipboardCopy { origin: [i32; 3], size: [i32; 3] },
    Verify { origin: [i32; 3] },
}
static PENDING: Mutex<Vec<PendingWorldCmd>> = Mutex::new(Vec::new());

/// The command hook: runs /litematica commands, claims only ours (any other
/// command passes through untouched, ran stays false).
pub fn on_command(command: &str, ran: &mut bool) {
    let Some(rest) = command.strip_prefix("/litematica") else {
        return;
    };
    let Some(parsed) = parse_command(rest) else {
        return;
    };
    *ran = true;
    match parsed {
        Command::Clear => clear(),
        Command::Status => report_status(),
        Command::Build => start_print(),
        Command::BuildStop => stop_print(),
        Command::Save { path } => {
            let _ = save_loaded_schematic(&path);
        }
        Command::SaveArea { origin, size, path } => {
            if let Ok(mut q) = PENDING.lock() {
                q.push(PendingWorldCmd::SaveArea { origin, size, path });
            }
            log::info!("litematica: savearea queued (runs next tick)");
        }
        Command::Clipboard { sub } => match sub {
            litematica_mod::commands::ClipboardSub::Copy => {
                match litematica_mod::commands::selection_box() {
                    Ok((origin, size)) => {
                        if let Ok(mut q) = PENDING.lock() {
                            q.push(PendingWorldCmd::ClipboardCopy { origin, size });
                        }
                        log::info!("litematica: clipboard copy queued (runs next tick)");
                    }
                    Err(e) => log::info!("litematica: {}", e),
                }
            }
            litematica_mod::commands::ClipboardSub::List => {
                log::info!("{}", litematica_mod::commands::clipboard_report())
            }
        }
        Command::SaveAreaFromSelection { path } => {
            match litematica_mod::commands::selection_box() {
                Ok((origin, size)) => {
                    if let Ok(mut q) = PENDING.lock() {
                        q.push(PendingWorldCmd::SaveArea { origin, size, path });
                    }
                    log::info!("litematica: savearea (selection) queued (runs next tick)");
                }
                Err(e) => log::info!("litematica: {}", e),
            }
        }
        Command::Verify { origin } => {
            let o = match origin {
                Some(o) => o,
                None => *LAST_LOAD_ORIGIN.lock().unwrap_or_else(|p| p.into_inner()),
            };
            if let Ok(mut q) = PENDING.lock() {
                q.push(PendingWorldCmd::Verify { origin: o });
            }
            log::info!("litematica: verify queued (runs next tick)");
        }
        Command::Select { sub } => {
            let msg = match sub {
                SelectSub::Pos1(p) => litematica_mod::commands::selection_set(1, Some(p)),
                SelectSub::Pos2(p) => litematica_mod::commands::selection_set(2, Some(p)),
                SelectSub::Clear => litematica_mod::commands::selection_set(1, None),
                SelectSub::Show => litematica_mod::commands::selection_report(),
            };
            log::info!("{}", msg);
        }
        Command::Materials => {
            match litematica_mod::commands::materials_report() {
                Some((total, lines)) => {
                    log::info!("litematica materials: {} total blocks, {} states", total, lines.len());
                    for l in lines.iter().take(50) {
                        log::info!("litematica materials: {}", l);
                    }
                    if lines.len() > 50 {
                        log::info!("litematica materials: ... {} more states", lines.len() - 50);
                    }
                }
                None => log::info!("litematica: nothing loaded"),
            }
        }
        Command::Load { path, origin } => {
            if let Ok(mut o) = LAST_LOAD_ORIGIN.lock() {
                *o = [origin.0, origin.1, origin.2];
            }
            let _ = load_schematic_file(&path, origin);
        }
    }
}


// ---- phase 4: the printer's runtime glue ----

/// Blocks the printer places per level tick.
pub const PRINT_BUDGET: usize = 64;

/// Vanilla's UPDATE_NEIGHBOURS | UPDATE_CLIENTS flags for placed blocks.
pub const PRINT_FLAGS: i32 = 3;

/// Print steps still to place, in build order.
static PRINT_QUEUE: Mutex<Vec<PrintStep>> = Mutex::new(Vec::new());

/// One print run's counters.
#[derive(Clone, Copy, Default)]
pub struct PrintStats {
    pub total: usize,
    pub placed: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// Counters for the current print run (latched by /litematica build).
static PRINT_STATS: Mutex<PrintStats> = Mutex::new(PrintStats {
    total: 0,
    placed: 0,
    skipped: 0,
    failed: 0,
});

/// Resolved state ids per schematic state string (registry lookups cached).
static STATE_CACHE: Mutex<Vec<(String, BlockStateId)>> = Mutex::new(Vec::new());

/// Queue the active overlay for printing (bottom-up build order).
pub fn start_print() {
    let ghosts = CURRENT.lock().map(|v| v.clone()).unwrap_or_default();
    let steps = print_order_ghosts(&ghosts);
    let total = steps.len();
    if let Ok(mut q) = PRINT_QUEUE.lock() {
        *q = steps;
    }
    if let Ok(mut s) = PRINT_STATS.lock() {
        *s = PrintStats { total, ..PrintStats::default() };
    }
    log::info!("litematica: print queued, {total} blocks");
}

/// Drop the print queue (the overlay stays).
pub fn stop_print() {
    if let Ok(mut q) = PRINT_QUEUE.lock() {
        q.clear();
    }
}

/// The print run's counters: (pending, total, placed, skipped, failed).
pub fn print_stats() -> (usize, usize, usize, usize, usize) {
    let pending = PRINT_QUEUE.lock().map(|q| q.len()).unwrap_or(0);
    let s = PRINT_STATS.lock().map(|s| *s).unwrap_or_default();
    (pending, s.total, s.placed, s.skipped, s.failed)
}

/// The /litematica status readout (the command hook has no chat channel, so
/// it logs).
fn report_status() {
    let (pending, total, placed, skipped, failed) = print_stats();
    log::info!(
        "litematica: {} ghosts loaded, print {}/{} queued, {} placed, {} already right, {} failed",
        len(), pending, total, placed, skipped, failed
    );
}

impl crate::game::Game<'_> {
    /// The level-tick hook: place up to PRINT_BUDGET queued print steps into
    /// the live world each tick pass. Native block writes are server-side, so
    /// the queue drains only while the game serves the level.
    pub fn litematica_level_tick(&mut self, dimension: &str) {
        if !self.native_on(ID) {
            return;
        }
        if self.serving.is_none() && self.options.dedicated.is_none() {
            return;
        }
        self.drain_pending();
        let steps: Vec<PrintStep> = match PRINT_QUEUE.lock() {
            Ok(mut q) => {
                if q.is_empty() {
                    return;
                }
                let n = q.len().min(PRINT_BUDGET);
                q.drain(..n).collect()
            }
            Err(_) => return,
        };
        let mut placed = 0usize;
        let mut skipped = 0usize;
        let mut failed = 0usize;
        for step in &steps {
            let Some(target) = self.litematica_state_id(&step.state) else {
                failed += 1;
                continue;
            };
            let (x, y, z) = step.world;
            if self.world.block(x, y, z).0 == target.0 {
                skipped += 1;
                continue;
            }
            if self.native_set_block(dimension, [x, y, z], target, PRINT_FLAGS) {
                placed += 1;
            } else {
                failed += 1;
            }
        }
        if let Ok(mut s) = PRINT_STATS.lock() {
            s.placed += placed;
            s.skipped += skipped;
            s.failed += failed;
        }
        if PRINT_QUEUE.lock().map(|q| q.len()).unwrap_or(0) == 0 {
            let s = PRINT_STATS.lock().map(|s| *s).unwrap_or_default();
            log::info!(
                "litematica: print finished, {} placed, {} already right, {} failed",
                s.placed, s.skipped, s.failed
            );
        }
    }

    /// Resolve a schematic state string ("minecraft:oak_planks[facing=north]")
    /// to a state id, cached; None when the world's registry doesn't know it.
    fn litematica_state_id(&self, key: &str) -> Option<BlockStateId> {
        if let Ok(cache) = STATE_CACHE.lock() {
            if let Some((_, id)) = cache.iter().find(|(k, _)| k == key) {
                return Some(*id);
            }
        }
        let mut reader = Reader::new(key);
        let parsed = block::parse(&mut reader, &self.ui.registries, false).ok()?;
        let id = self.block_state(&parsed).ok()?;
        if let Ok(mut cache) = STATE_CACHE.lock() {
            cache.push((key.to_string(), id));
        }
        Some(id)
    }
}

impl crate::game::Game<'_> {
    /// World-reading commands queued by the command hook, executed here where
    /// the Game is in hand.
    pub fn drain_pending(&mut self) {
        let pending = match PENDING.lock() {
            Ok(mut q) => std::mem::take(&mut *q),
            Err(_) => return,
        };
        for cmd in pending {
            match cmd {
                PendingWorldCmd::Verify { origin } => {
                    match self.verify_area_from_world((origin[0], origin[1], origin[2])) {
                        Ok(summary) => log::info!("litematica verify: {}", summary),
                        Err(e) => log::info!("litematica: {}", e),
                    }
                }
                PendingWorldCmd::ClipboardCopy { origin, size } => {
                    match self.read_area_states(origin, size) {
                        Ok(states) => log::info!(
                            "{}",
                            litematica_mod::commands::clipboard_store(origin, size, states)
                        ),
                        Err(e) => log::info!("litematica: {}", e),
                    }
                }
                PendingWorldCmd::SaveArea { origin, size, path } => {
                    match self.save_area_from_world(origin, size, &path) {
                        Ok(n) => log::info!("litematica savearea: wrote {} region(s) to {}", n, path),
                        Err(e) => log::info!("litematica: {}", e),
                    }
                }
            }
        }
    }

    /// Compare the last loaded schematic (placed at `origin`) against the live
    /// world. Every non-air expected block whose state resolves is checked;
    /// unresolved states are skipped. Live blocks that did not load read as
    /// air (id 0), which reports as missing.
    pub fn verify_area_from_world(&self, origin: (i32, i32, i32)) -> Result<String, String> {
        let sch = litematica_mod::commands::last_schematic().ok_or_else(|| "litematica: nothing loaded".to_string())?;
        let mut checked = 0u64;
        let mut wrong = 0u64;
        let mut missing = 0u64;
        let mut details: Vec<String> = Vec::new();
        for (name, region) in &sch.regions {
            let (w, h, l) = region.dims();
            for y in 0..h {
                for z in 0..l {
                    for x in 0..w {
                        let state = region.get(x, y, z);
                        if state.name == "minecraft:air" && state.properties.is_empty() {
                            continue;
                        }
                        let key = state.key();
                        let Some(target) = self.litematica_state_id(&key) else {
                            continue;
                        };
                        let wx = origin.0 + x as i32;
                        let wy = origin.1 + y as i32;
                        let wz = origin.2 + z as i32;
                        let live = self.world.block(wx, wy, wz).0;
                        checked += 1;
                        if live == target.0 {
                            continue;
                        }
                        if live == 0 {
                            missing += 1;
                        } else {
                            wrong += 1;
                        }
                        if details.len() < 5 {
                            details.push(format!(
                                "{} @ ({}, {}, {}): expected {}, found id {}",
                                name, wx, wy, wz, key, live
                            ));
                        }
                    }
                }
            }
        }
        if details.is_empty() {
            Ok(format!("{} checked, all match", checked))
        } else {
            Ok(format!(
                "{} checked, {} wrong, {} missing; first details:\n  {}",
                checked, wrong, missing, details.join("\n  ")
            ))
        }
    }

    /// Capture a live-world box into a gzipped .litematic file. Block ids map
    /// back to state keys through the game's block registry.
    /// Read a box of world block-state keys, row-major (y, z, x); None = air.
    /// Shared by save-area and the clipboard copy.
    pub fn read_area_states(
        &self,
        origin: [i32; 3],
        size: [i32; 3],
    ) -> Result<Vec<Option<String>>, String> {
        let (w, h, l) = (size[0], size[1], size[2]);
        if w <= 0 || h <= 0 || l <= 0 {
            return Err("litematica: size must be positive".to_string());
        }
        if w as i64 * h as i64 * l as i64 > 8_388_608 {
            return Err("litematica: box too large (cap 8,388,608 blocks)".to_string());
        }
        let registry = &self.pack.assets.registry;
        let mut keys: Vec<Option<String>> = Vec::new();
        for y in 0..h {
            for z in 0..l {
                for x in 0..w {
                    let id = self.world.block(origin[0] + x, origin[1] + y, origin[2] + z).0;
                    let key = match registry.state(horizon_world::registry::BlockStateId(id)) {
                        Some(st) => {
                            let mut key = st.name.clone();
                            if !st.properties.is_empty() {
                                let parts: Vec<String> = st
                                    .properties
                                    .iter()
                                    .map(|(k, v)| format!("{}={}", k, v))
                                    .collect();
                                key = format!("{}[{}]", key, parts.join(","));
                            }
                            key
                        }
                        None => "minecraft:air".to_string(),
                    };
                    if key == "minecraft:air" {
                        keys.push(None);
                    } else {
                        keys.push(Some(key));
                    }
                }
            }
        }
        Ok(keys)
    }

    pub fn save_area_from_world(&self, origin: [i32; 3], size: [i32; 3], path: &str) -> Result<usize, String> {
        let (w, h, l) = (size[0], size[1], size[2]);
        if w <= 0 || h <= 0 || l <= 0 {
            return Err("litematica: size must be positive".to_string());
        }
        if w as i64 * h as i64 * l as i64 > 8_388_608 {
            return Err("litematica: box too large (cap 8,388,608 blocks)".to_string());
        }
        let keys = self.read_area_states(origin, size)?;
        let sch = litematica_mod::litematic::schematic_from_states(
            "savearea",
            (origin[0], origin[1], origin[2]),
            (w, h, l),
            &keys,
        )
        .ok_or_else(|| "litematica: could not build the schematic from the world".to_string())?;
        let bytes = litematica_mod::litematic::gzip_stored(&sch.to_bytes());
        std::fs::write(path, &bytes).map_err(|e| format!("litematica: cannot write {path}: {e}"))?;
        Ok(1usize)
    }
}
