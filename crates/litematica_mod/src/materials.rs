//! Phase 2, slice 1: material lists (what a schematic needs, by block state),
//! ported from litematica's materials/ package (LGPL-3.0, maruohon/litematica).

use crate::litematic::{BlockState, Schematic};
use std::collections::BTreeMap;

impl BlockState {
    /// Canonical key for counting: `name[prop=v,prop=v]`, properties sorted.
    pub fn key(&self) -> String {
        if self.properties.is_empty() {
            self.name.clone()
        } else {
            let props: Vec<String> =
                self.properties.iter().map(|(k, v)| format!("{k}={v}")).collect();
            format!("{}[{}]", self.name, props.join(","))
        }
    }
}

/// One material line: how many of a block state the schematic needs, and how
/// many of those are already placed by a partially built copy (always 0 here;
/// the verifier fills it in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterialLine {
    pub state: String,
    pub count: u64,
}

/// Aggregate the material list of a schematic: every non-air block state,
/// counted across all regions. Sorted by state key for stable output.
pub fn material_list(schematic: &Schematic) -> Vec<MaterialLine> {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for region in schematic.regions.values() {
        let (w, h, l) = region.dims();
        for y in 0..h {
            for z in 0..l {
                for x in 0..w {
                    let state = region.get(x, y, z);
                    if state.name == "minecraft:air" && state.properties.is_empty() {
                        continue;
                    }
                    *counts.entry(state.key()).or_insert(0) += 1;
                }
            }
        }
    }
    counts
        .into_iter()
        .map(|(state, count)| MaterialLine { state, count })
        .collect()
}

/// Total non-air blocks, for progress math.
pub fn total_blocks(schematic: &Schematic) -> u64 {
    material_list(schematic).iter().map(|l| l.count).sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::litematic::{Region, Schematic};
    use std::collections::BTreeMap;

    fn state(name: &str) -> BlockState {
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
    fn counts_by_state_across_regions() {
        let mut sch = Schematic::new("t", "a", "g", 0);
        // Region 1: 2x1x1, stone + air.
        let r1 = region_with(
            vec![state("minecraft:stone"), state("minecraft:air")],
            vec![0, 1],
            (2, 1, 1),
        );
        // Region 2: 1x1x1, stone.
        let r2 = region_with(vec![state("minecraft:stone")], vec![0], (1, 1, 1));
        sch.add_region("r1", r1);
        sch.add_region("r2", r2);
        let list = material_list(&sch);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].state, "minecraft:stone");
        assert_eq!(list[0].count, 2);
        assert_eq!(total_blocks(&sch), 2);
    }

    #[test]
    fn states_with_properties_count_separately() {
        let mut props = BTreeMap::new();
        props.insert("facing".to_string(), "north".to_string());
        let facing = BlockState { name: "minecraft:oak_stairs".into(), properties: props };
        let mut sch = Schematic::new("t", "a", "g", 0);
        sch.add_region(
            "r",
            region_with(vec![state("minecraft:oak_stairs"), facing], vec![0, 1], (2, 1, 1)),
        );
        let list = material_list(&sch);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].state, "minecraft:oak_stairs");
        assert_eq!(list[1].state, "minecraft:oak_stairs[facing=north]");
        assert_eq!(list[1].count, 1);
    }

    #[test]
    fn air_is_not_a_material() {
        let mut sch = Schematic::new("t", "a", "g", 0);
        sch.add_region("r", region_with(vec![state("minecraft:air")], vec![0; 12], (3, 2, 2)));
        assert!(material_list(&sch).is_empty());
        assert_eq!(total_blocks(&sch), 0);
    }
}
