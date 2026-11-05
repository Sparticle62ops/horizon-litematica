//! Phase 2, slice 2a: the schematic verifier (missing / wrong block / wrong
//! state / extra), ported from litematica's schematic/verifier package
//! (LGPL-3.0, maruohon/litematica).

use crate::litematic::Region;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MismatchKind {
    /// Expected a non-air block, found air.
    Missing,
    /// Both non-air, different block names.
    WrongBlock,
    /// Same block name, different properties.
    WrongState,
    /// Expected air, found a non-air block.
    Extra,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    pub kind: MismatchKind,
    /// Region-local position (x, y, z).
    pub pos: (usize, usize, usize),
    /// Canonical state key of what the schematic wants (air where Extra).
    pub expected: String,
    /// Canonical state key of what is actually there (air where Missing).
    pub found: String,
}

/// Compare a placed region against the schematic region. Both must have the
/// same dimensions; returns Err for a size mismatch. Air is
/// `minecraft:air` with no properties, matching the reader's default.
pub fn verify(placed: &Region, schematic: &Region) -> Result<Vec<Mismatch>, String> {
    if placed.dims() != schematic.dims() {
        return Err(format!(
            "dimension mismatch: placed {:?} vs schematic {:?}",
            placed.dims(),
            schematic.dims()
        ));
    }
    let (w, h, l) = schematic.dims();
    let mut out = Vec::new();
    for y in 0..h {
        for z in 0..l {
            for x in 0..w {
                let want = schematic.get(x, y, z);
                let have = placed.get(x, y, z);
                if want.name == have.name && want.properties == have.properties {
                    continue;
                }
                let want_air = want.name == "minecraft:air" && want.properties.is_empty();
                let have_air = have.name == "minecraft:air" && have.properties.is_empty();
                let kind = match (want_air, have_air) {
                    (false, true) => MismatchKind::Missing,
                    (true, false) => MismatchKind::Extra,
                    (false, false) if want.name != have.name => MismatchKind::WrongBlock,
                    (false, false) => MismatchKind::WrongState,
                    (true, true) => unreachable!(),
                };
                out.push(Mismatch {
                    kind,
                    pos: (x, y, z),
                    expected: want.key(),
                    found: have.key(),
                });
            }
        }
    }
    Ok(out)
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

    fn schema() -> Region {
        // 2x1x1: stone, oak_planks.
        region_with(
            vec![st("minecraft:stone"), st("minecraft:oak_planks")],
            vec![0, 1],
            (2, 1, 1),
        )
    }

    #[test]
    fn clean_build_has_no_mismatches() {
        let placed = schema();
        assert!(verify(&placed, &schema()).unwrap().is_empty());
    }

    #[test]
    fn missing_and_extra_are_flagged() {
        // air, air where schema wants stone, planks.
        let placed = region_with(
            vec![st("minecraft:stone"), st("minecraft:oak_planks"), st("minecraft:air")],
            vec![2, 2],
            (2, 1, 1),
        );
        let ms = verify(&placed, &schema()).unwrap();
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].kind, MismatchKind::Missing);
        assert_eq!(ms[0].pos, (0, 0, 0));
        assert_eq!(ms[0].expected, "minecraft:stone");
        assert_eq!(ms[1].kind, MismatchKind::Missing);
        assert_eq!(ms[1].pos, (1, 0, 0));
    }

    #[test]
    fn wrong_block_and_wrong_state_distinguished() {
        let mut props = BTreeMap::new();
        props.insert("facing".to_string(), "north".to_string());
        let facing = BlockState { name: "minecraft:oak_stairs".into(), properties: props };
        // Schema: stone, oak_stairs[facing=north]. Placed: cobblestone at 0
        // (wrong block), oak_stairs[facing=south] at 1 (wrong state).
        let mut wrong_props = BTreeMap::new();
        wrong_props.insert("facing".to_string(), "south".to_string());
        let wrong_state = BlockState { name: "minecraft:oak_stairs".into(), properties: wrong_props };
        let schema = region_with(
            vec![st("minecraft:stone"), facing.clone()],
            vec![0, 1],
            (2, 1, 1),
        );
        let placed = region_with(
            vec![st("minecraft:cobblestone"), facing, wrong_state],
            vec![0, 2],
            (2, 1, 1),
        );
        let ms = verify(&placed, &schema).unwrap();
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].kind, MismatchKind::WrongBlock);
        assert_eq!(ms[0].found, "minecraft:cobblestone");
        assert_eq!(ms[1].kind, MismatchKind::WrongState);
        assert_eq!(ms[1].expected, "minecraft:oak_stairs[facing=north]");
        assert_eq!(ms[1].found, "minecraft:oak_stairs[facing=south]");
    }

    #[test]
    fn size_mismatch_is_an_error() {
        let placed = region_with(vec![st("minecraft:stone")], vec![0], (1, 1, 1));
        assert!(verify(&placed, &schema()).is_err());
    }
}
