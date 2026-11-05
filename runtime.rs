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
use litematica_mod::commands::{load_schematic_file, parse_command, Command};

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
        Command::Status => { let _ = len(); }
        Command::Load { path, origin } => {
            let _ = load_schematic_file(&path, origin);
        }
    }
}
