//! Phase 2, slice 2b: placements, ported from litematica's
//! schematic/placement + util/position transforms (LGPL-3.0,
//! maruohon/litematica). Covers the pure-logic transforms: rotate/mirror of a
//! region's contents, with block-state property remapping for the properties
//! that encode orientation, and a positional move.

use crate::litematic::{BlockState, Region};
use std::collections::{BTreeMap, HashMap};

/// The orientation-encoding properties litematica remaps on rotate/mirror.
/// `facing` is the directional property (stairs, pistons, observers);
/// `rotation` is the 16-step rotation (signs, banners).
fn remap_props(props: &BTreeMap<String, String>, turns: u32, mirror_x: bool, mirror_z: bool) -> BTreeMap<String, String> {
    let mut out = props.clone();
    let turns = turns % 4;
    if let Some(f) = out.get_mut("facing") {
        // CW: north -> east -> south -> west. With no rotation, a mirror
        // across X swaps east/west and across Z swaps north/south.
        if let Some(i) = ["north", "east", "south", "west"].iter().position(|d| d == f) {
            if turns > 0 {
                *f = ["north", "east", "south", "west"][(i + turns as usize) % 4].to_string();
            } else if mirror_x && (i == 1 || i == 3) {
                *f = if *f == "east" { "west".into() } else { "east".into() };
            } else if mirror_z && (i == 0 || i == 2) {
                *f = if *f == "north" { "south".into() } else { "north".into() };
            }
        }
    }
    if let Some(r) = out.get_mut("rotation") {
        if let Ok(v) = r.parse::<u32>() {
            if v < 16 {
                if turns > 0 {
                    let nv = (v + 4 * turns) % 16;
                    *r = nv.to_string();
                } else if mirror_x || mirror_z {
                    let nv = (16 - v) % 16;
                    *r = nv.to_string();
                }
            }
        }
    }
    out
}

/// Transformed copy of a region: rotate `turns` quarter-turns clockwise
/// (viewed from above), then optionally mirror on the X or Z axis. Positional
/// origin stays; use `moved` for world placement.
pub fn transform(region: &Region, turns: u32, mirror_x: bool, mirror_z: bool) -> Region {
    let (w, h, l) = region.dims();
    let turns = turns % 4;
    let (nw, nl) = if turns % 2 == 1 { (l, w) } else { (w, l) };
    let volume = nw * h * nl;
    let mut new_palette: Vec<BlockState> = Vec::new();
    let mut index_map: HashMap<u32, u32> = HashMap::new();
    let mut blocks = vec![0u32; volume];
    for y in 0..h {
        for z in 0..l {
            for x in 0..w {
                let src = region.blocks[region.index_of(x, y, z)];
                let dst_idx = match index_map.get(&src) {
                    Some(i) => *i,
                    None => {
                        let state = &region.palette[src as usize];
                        let new_state = BlockState {
                            name: state.name.clone(),
                            properties: remap_props(&state.properties, turns, mirror_x, mirror_z),
                        };
                        let i = new_palette.len() as u32;
                        new_palette.push(new_state);
                        index_map.insert(src, i);
                        i
                    }
                };
                // Target coords: mirror first (within source dims), then rotate CW.
                let mut tx = if mirror_x { w - 1 - x } else { x };
                let mut tz = if mirror_z { l - 1 - z } else { z };
                let mut tw = w;
                let mut tl = l;
                for _ in 0..turns {
                    let nx = tl - 1 - tz;
                    tz = tx;
                    tx = nx;
                    let old_w = tw;
                    tw = tl;
                    tl = old_w;
                }
                let target = index_in(nw, h, nl, tx, y, tz);
                debug_assert!(tx < tw && tz < tl);
                blocks[target] = dst_idx;
            }
        }
    }
    Region {
        position: region.position,
        size: (nw as i32, region.size.1, nl as i32),
        palette: new_palette,
        blocks,
        tile_entities: region.tile_entities.clone(),
        entities: region.entities.clone(),
    }
}

/// x fastest, then z, then y, matching Region::index_of.
fn index_in(w: usize, _h: usize, l: usize, x: usize, y: usize, z: usize) -> usize {
    y * (w * l) + z * w + x
}

/// A region placed at a world position, i.e. the placement record
/// litematica keeps for each schematic copy.
#[derive(Debug, Clone)]
pub struct Placement {
    /// Region-local schematic region.
    pub region: Region,
    /// World block position of the region's (0, 0, 0) corner.
    pub origin: (i32, i32, i32),
    /// Transform applied to the region (already baked into `region` by
    /// `transform`; recorded here for the overlay/printer).
    pub turns: u32,
    pub mirror_x: bool,
    pub mirror_z: bool,
}

impl Placement {
    pub fn new(region: Region, origin: (i32, i32, i32), turns: u32, mirror_x: bool, mirror_z: bool) -> Self {
        Self { region, origin, turns: turns % 4, mirror_x, mirror_z }
    }

    /// Region-local (x, y, z) -> world position.
    pub fn world_pos(&self, x: usize, y: usize, z: usize) -> (i32, i32, i32) {
        (
            self.origin.0 + x as i32,
            self.origin.1 + y as i32,
            self.origin.2 + z as i32,
        )
    }

    /// Every non-air cell of this placement, as (world pos, state key).
    pub fn world_blocks(&self) -> Vec<((i32, i32, i32), String)> {
        let (w, h, l) = self.region.dims();
        let mut out = Vec::new();
        for y in 0..h {
            for z in 0..l {
                for x in 0..w {
                    let state = self.region.get(x, y, z);
                    if state.name == "minecraft:air" && state.properties.is_empty() {
                        continue;
                    }
                    out.push((self.world_pos(x, y, z), state.key()));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::litematic::BlockState;

    fn st(name: &str) -> BlockState {
        BlockState { name: name.into(), properties: BTreeMap::new() }
    }

    fn facing(name: &str, dir: &str) -> BlockState {
        let mut props = BTreeMap::new();
        props.insert("facing".to_string(), dir.to_string());
        BlockState { name: name.into(), properties: props }
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
    fn rotate_cw_moves_nw_corner_to_ne() {
        // 2x1x3: unique states A, B, C, D, E, F laid out x-major.
        let pal = vec![
            st("minecraft:a"), st("minecraft:b"), st("minecraft:c"),
            st("minecraft:d"), st("minecraft:e"), st("minecraft:f"),
        ];
        let region = region_with(pal, (0..6).collect(), (2, 1, 3));
        let rotated = transform(&region, 1, false, false);
        assert_eq!(rotated.dims(), (3, 1, 2));
        // Source (0,0,0) (state a) is the NW corner; CW rotation maps it to
        // the NE corner of the 3x2 footprint, i.e. (2, 0, 0).
        assert_eq!(rotated.get(2, 0, 0).name, "minecraft:a");
        // Source (1,0,0) (b) -> x'=2, z'=1.
        assert_eq!(rotated.get(2, 0, 1).name, "minecraft:b");
        // Source (0,0,2) (e, x=0,z=2) -> x'=0, z'=0.
        assert_eq!(rotated.get(0, 0, 0).name, "minecraft:e");
    }

    #[test]
    fn four_rotations_return_the_original() {
        let pal = vec![st("minecraft:stone"), st("minecraft:oak_planks"), st("minecraft:air")];
        let region = region_with(pal, vec![0, 1, 2, 1, 0, 2], (3, 1, 2));
        let mut cur = region.clone();
        for _ in 0..4 {
            cur = transform(&cur, 1, false, false);
        }
        for y in 0..1 {
            for z in 0..2 {
                for x in 0..3 {
                    assert_eq!(cur.get(x, y, z).name, region.get(x, y, z).name);
                }
            }
        }
    }

    #[test]
    fn facing_remaps_on_rotation_and_mirror() {
        let pal = vec![facing("minecraft:oak_stairs", "north")];
        let region = region_with(pal, vec![0], (1, 1, 1));
        let east = transform(&region, 1, false, false);
        assert_eq!(east.get(0, 0, 0).key(), "minecraft:oak_stairs[facing=east]");
        let west = transform(&region, 2, false, false);
        assert_eq!(west.get(0, 0, 0).key(), "minecraft:oak_stairs[facing=south]");
        let mirrored = transform(&region, 0, true, false);
        // Mirror X keeps north/south facings.
        assert_eq!(mirrored.get(0, 0, 0).key(), "minecraft:oak_stairs[facing=north]");
        let diag = facing("minecraft:oak_stairs", "east");
        let m2 = region_with(vec![diag], vec![0], (1, 1, 1));
        let mirrored_east = transform(&m2, 0, true, false);
        assert_eq!(mirrored_east.get(0, 0, 0).key(), "minecraft:oak_stairs[facing=west]");
    }

    #[test]
    fn placement_world_blocks_skip_air_and_offset() {
        let pal = vec![st("minecraft:stone"), st("minecraft:air")];
        let region = region_with(pal, vec![0, 1], (2, 1, 1));
        let placement = Placement::new(region, (100, 64, -5), 0, false, false);
        let blocks = placement.world_blocks();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0], ((100, 64, -5), "minecraft:stone".to_string()));
    }
}
