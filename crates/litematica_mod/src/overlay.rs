//! Phase 3, slice 1: the ghost-block overlay's pure-logic half, ported from
//! litematica's render/schematic overlay data (LGPL-3.0, maruohon/litematica).
//! Picks which world positions get an overlay primitive and what it means;
//! the Horizon glue draws them with WorldPrimitives' filled boxes and
//! outlines (phase 3, slice 2).

use crate::litematic::Region;
use crate::placements::Placement;
use crate::verify::{self, MismatchKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    /// Block the schematic wants and nothing is there: drawn as a ghost.
    Missing,
    /// Wrong block or wrong state there: highlighted.
    Wrong,
    /// A block there that the schematic does not want: highlighted.
    Extra,
}

/// One overlay primitive: a world position and what kind of overlay it gets,
/// with the state key the ghost stands for (empty for Extra).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhostBlock {
    pub world: (i32, i32, i32),
    pub kind: OverlayKind,
    pub state_key: String,
}

/// Default ARGB colors for the Horizon glue (translucent, WorldPrimitives
/// blend over the world).
pub const COLOR_MISSING: u32 = 0x64FF_FF_FF;
pub const COLOR_WRONG: u32 = 0x64FF_A500;
pub const COLOR_EXTRA: u32 = 0x96FF_2020;

pub fn color_for(kind: OverlayKind) -> u32 {
    match kind {
        OverlayKind::Missing => COLOR_MISSING,
        OverlayKind::Wrong => COLOR_WRONG,
        OverlayKind::Extra => COLOR_EXTRA,
    }
}

/// The overlay for one placement. With no placed region, every non-air block
/// of the schematic is a Missing ghost (the placement preview). With a placed
/// region of the same dims, the verifier's mismatches drive it.
pub fn overlay_for(placement: &Placement, placed: Option<&Region>) -> Vec<GhostBlock> {
    let mut out = Vec::new();
    match placed {
        None => {
            for (world, state_key) in placement.world_blocks() {
                out.push(GhostBlock { world, kind: OverlayKind::Missing, state_key });
            }
        }
        Some(placed) => {
            if let Ok(mismatches) = verify::verify(placed, &placement.region) {
                for m in mismatches {
                    let kind = match m.kind {
                        MismatchKind::Missing => OverlayKind::Missing,
                        MismatchKind::WrongBlock | MismatchKind::WrongState => OverlayKind::Wrong,
                        MismatchKind::Extra => OverlayKind::Extra,
                    };
                    out.push(GhostBlock {
                        world: placement.world_pos(m.pos.0, m.pos.1, m.pos.2),
                        kind,
                        state_key: match m.kind {
                            MismatchKind::Extra => String::new(),
                            _ => m.expected,
                        },
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::litematic::BlockState;
    use std::collections::BTreeMap;

    fn st(name: &str) -> BlockState {
        BlockState { name: name.into(), properties: BTreeMap::new() }
    }

    fn region_with(palette: Vec<BlockState>, blocks: Vec<u32>, size: (i32, i32, i32)) -> Region {
        Region {
            position: (0, 0, 0),
            size,
            palette,
            blocks,
            tile_entities: Vec::new(),
            entities: Vec::new(),
        }
    }

    #[test]
    fn preview_marks_every_non_air_block() {
        let region = region_with(
            vec![st("minecraft:stone"), st("minecraft:air")],
            vec![0, 1],
            (2, 1, 1),
        );
        let placement = Placement::new(region, (10, 64, 10), 0, false, false);
        let ghosts = overlay_for(&placement, None);
        assert_eq!(ghosts.len(), 1);
        assert_eq!(ghosts[0].kind, OverlayKind::Missing);
        assert_eq!(ghosts[0].world, (10, 64, 10));
        assert_eq!(ghosts[0].state_key, "minecraft:stone");
        assert_eq!(color_for(OverlayKind::Missing), COLOR_MISSING);
    }

    #[test]
    fn mismatches_map_to_kinds() {
        // Schema: stone, planks. Placed: air, cobble.
        let region = region_with(
            vec![st("minecraft:stone"), st("minecraft:oak_planks")],
            vec![0, 1],
            (2, 1, 1),
        );
        let placement = Placement::new(region, (0, 0, 0), 0, false, false);
        let placed = region_with(
            vec![st("minecraft:stone"), st("minecraft:oak_planks"), st("minecraft:air"), st("minecraft:cobblestone")],
            vec![2, 3],
            (2, 1, 1),
        );
        let ghosts = overlay_for(&placement, Some(&placed));
        assert_eq!(ghosts.len(), 2);
        assert_eq!(ghosts[0].kind, OverlayKind::Missing);
        assert_eq!(ghosts[0].state_key, "minecraft:stone");
        assert_eq!(ghosts[1].kind, OverlayKind::Wrong);
        assert_eq!(color_for(OverlayKind::Wrong), COLOR_WRONG);
    }

    #[test]
    fn extra_gets_the_extra_color_without_a_state() {
        // Schema: 1x1x1 air. Placed: stone.
        let region = region_with(vec![st("minecraft:air")], vec![0], (1, 1, 1));
        let placement = Placement::new(region, (5, 5, 5), 0, false, false);
        let placed = region_with(vec![st("minecraft:stone")], vec![0], (1, 1, 1));
        let ghosts = overlay_for(&placement, Some(&placed));
        assert_eq!(ghosts.len(), 1);
        assert_eq!(ghosts[0].kind, OverlayKind::Extra);
        assert!(ghosts[0].state_key.is_empty());
        assert_eq!(color_for(OverlayKind::Extra), COLOR_EXTRA);
    }
}
