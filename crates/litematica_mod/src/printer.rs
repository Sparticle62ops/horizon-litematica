//! Print order logic, ported from maruohon/litematica (LGPL-3.0).
//! The printer walks a placement bottom-up so each placed block rests on
//! the layer under it, and re-checks the live world so it never re-places
//! a block that is already right.

use crate::overlay::GhostBlock;
use crate::placements::Placement;

/// One block the printer wants to place, at its world position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrintStep {
    pub world: (i32, i32, i32),
    pub state: String,
}

/// The full build order for a placement: bottom-up (y ascending), then x,
/// then z inside each layer.
pub fn print_order(placement: &Placement) -> Vec<PrintStep> {
    let mut steps: Vec<PrintStep> = placement
        .world_blocks()
        .into_iter()
        .map(|(world, state)| PrintStep { world, state })
        .collect();
    steps.sort_by_key(|s| (s.world.1, s.world.0, s.world.2));
    steps
}

/// Whether this step still needs a placement given the block the world
/// currently holds at its position (None = air or nothing).
pub fn needs_place(current: Option<&str>, step: &PrintStep) -> bool {
    match current {
        None => true,
        Some(c) => c != step.state,
    }
}

/// The steps of `order` the world still needs, given a lookup of the
/// current block per world position.
pub fn pending_steps<F>(order: &[PrintStep], mut current_at: F) -> Vec<PrintStep>
where
    F: FnMut((i32, i32, i32)) -> Option<String>,
{
    order
        .iter()
        .filter(|s| needs_place(current_at(s.world).as_deref(), s))
        .cloned()
        .collect()
}

/// The build order for an active ghost overlay: bottom-up (y ascending),
/// then x, then z inside each layer.
pub fn print_order_ghosts(ghosts: &[GhostBlock]) -> Vec<PrintStep> {
    let mut steps: Vec<PrintStep> = ghosts
        .iter()
        .map(|g| PrintStep { world: g.world, state: g.state_key.clone() })
        .collect();
    steps.sort_by_key(|s| (s.world.1, s.world.0, s.world.2));
    steps
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::litematic::{BlockState, Region};
    use std::collections::BTreeMap;
    use crate::overlay::OverlayKind;

    #[test]
    fn print_order_ghosts_sorts_bottom_up() {
        let g = |x: i32, y: i32, z: i32, s: &str| GhostBlock {
            world: (x, y, z),
            kind: OverlayKind::Missing,
            state_key: s.to_string(),
        };
        let steps = print_order_ghosts(&[g(1, 0, 0, "a"), g(0, 1, 0, "b"), g(0, 0, 1, "c")]);
        let names: Vec<&str> = steps.iter().map(|s| s.state.as_str()).collect();
        assert_eq!(names, vec!["c", "a", "b"]);
    }

    fn st(name: &str) -> BlockState {
        BlockState { name: name.to_string(), properties: BTreeMap::new() }
    }

    fn region_with(
        palette: Vec<BlockState>,
        blocks: Vec<u32>,
        size: (usize, usize, usize),
    ) -> Region {
        Region {
            position: (0, 0, 0),
            size: (size.0 as i32, size.1 as i32, size.2 as i32),
            palette,
            blocks,
            tile_entities: vec![],
            entities: vec![],
        }
    }

    #[test]
    fn order_is_bottom_up() {
        // 1x2x1: bottom dirt, top stone; the printer must place the dirt first.
        let region = region_with(vec![st("minecraft:dirt"), st("minecraft:stone")], vec![0, 1], (1, 2, 1));
        let placement = Placement::new(region, (10, 64, -3), 0, false, false);
        let steps = print_order(&placement);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0], PrintStep { world: (10, 64, -3), state: "minecraft:dirt".into() });
        assert_eq!(steps[1], PrintStep { world: (10, 65, -3), state: "minecraft:stone".into() });
    }

    #[test]
    fn order_skips_air() {
        let region = region_with(vec![st("minecraft:stone"), st("minecraft:air")], vec![0, 1], (2, 1, 1));
        let placement = Placement::new(region, (0, 0, 0), 0, false, false);
        let steps = print_order(&placement);
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].state, "minecraft:stone");
    }

    #[test]
    fn order_sorts_x_then_z_inside_a_layer() {
        // 2x1x2 fully filled; inside the layer the order is x, then z.
        let palette = vec![
            st("minecraft:stone"),
            st("minecraft:dirt"),
            st("minecraft:brick_block"),
            st("minecraft:gold_block"),
        ];
        // index_of: x fastest, then z, then y.
        let region = region_with(palette, vec![0, 1, 2, 3], (2, 1, 2));
        let placement = Placement::new(region, (0, 0, 0), 0, false, false);
        let steps = print_order(&placement);
        let states: Vec<&str> = steps.iter().map(|s| s.state.as_str()).collect();
        assert_eq!(states, vec!["minecraft:stone", "minecraft:brick_block", "minecraft:dirt", "minecraft:gold_block"]);
    }

    #[test]
    fn needs_place_checks_the_world_block() {
        let step = PrintStep { world: (0, 0, 0), state: "minecraft:stone".into() };
        assert!(needs_place(None, &step));
        assert!(needs_place(Some("minecraft:dirt"), &step));
        assert!(!needs_place(Some("minecraft:stone"), &step));
    }

    #[test]
    fn pending_steps_keep_order_and_drop_satisfied_ones() {
        let region = region_with(vec![st("minecraft:dirt"), st("minecraft:stone")], vec![0, 1], (1, 2, 1));
        let placement = Placement::new(region, (0, 0, 0), 0, false, false);
        let order = print_order(&placement);
        let world = |pos: (i32, i32, i32)| {
            if pos.1 == 0 {
                Some("minecraft:dirt".to_string()) // already placed correctly
            } else {
                None // still air
            }
        };
        let pending = pending_steps(&order, world);
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].state, "minecraft:stone");
    }
}
