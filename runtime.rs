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
use litematica_mod::commands::{load_schematic_file, save_loaded_schematic, parse_command, Command};
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
            let _ = save_area_from_world(self, origin, size, &path);
        }
        Command::Verify { origin } => {
            let o = match origin {
                Some(o) => o,
                None => *LAST_LOAD_ORIGIN.lock().unwrap_or_else(|p| p.into_inner()),
            };
            match verify_area_from_world(self, (o[0], o[1], o[2])) {
                Ok(summary) => log::info!("litematica verify: {}", summary),
                Err(e) => log::info!("litematica: {}", e),
            }
        }
        Command::Materials => {
            match commands::materials_report() {
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
                *o = origin;
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
