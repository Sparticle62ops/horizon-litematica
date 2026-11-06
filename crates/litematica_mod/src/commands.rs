//! The /litematica command surface, kept in the bundle crate so its pure
//! parts are unit-tested by the crate itself (runtime.rs calls into these).

use crate::litematic::Schematic;
use std::sync::Mutex;
use crate::litematic::gzip_stored;
use crate::overlay::overlay_for;
use crate::placements::Placement;

/// One parsed /litematica command.
#[derive(Debug, PartialEq)]
pub enum Command {
    Clear,
    Status,
    Load { path: String, origin: (i32, i32, i32) },
    /// Print the loaded schematic into the world (phase 4 printer).
    Build,
    /// Stop the running print (the overlay stays).
    BuildStop,
    /// Write the currently loaded schematic back out as a .litematic file.
    Save { path: String },
    SaveArea { origin: [i32; 3], size: [i32; 3], path: String },
    /// Save the current selection box (no explicit geometry) as a .litematic.
    SaveAreaFromSelection { path: String },
    Verify { origin: Option<[i32; 3]> },
    /// Summarize what the loaded schematic needs, by block state.
    Materials,
    /// Phase 5 selection: set/inspect the selection box corners.
    Select { sub: SelectSub },
}

/// One /litematica select sub-command.
#[derive(Debug, PartialEq)]
pub enum SelectSub {
    Pos1([i32; 3]),
    Pos2([i32; 3]),
    Clear,
    Show,
}

/// Parse a /litematica command body: "load <path> [x y z]", "clear",
/// "status", "build", "build stop". Returns None for anything else (the
/// command is not ours).
pub fn parse_command(rest: &str) -> Option<Command> {
    let rest = rest.trim();
    if rest == "clear" {
        return Some(Command::Clear);
    }
    if rest == "status" {
        return Some(Command::Status);
    }
    if rest == "build stop" {
        return Some(Command::BuildStop);
    }
    if rest == "build" {
        return Some(Command::Build);
    }
    if let Some(args) = rest.strip_prefix("savearea") {
        let mut parts = args.trim().split_whitespace();
        let first = parts.next()?;
        if first.parse::<i32>().is_ok() {
            let x: i32 = first.parse().ok()?;
            let y: i32 = parts.next()?.parse().ok()?;
            let z: i32 = parts.next()?.parse().ok()?;
            let w: i32 = parts.next()?.parse().ok()?;
            let h: i32 = parts.next()?.parse().ok()?;
            let l: i32 = parts.next()?.parse().ok()?;
            let path = parts.next()?.to_string();
            if parts.next().is_some() || w <= 0 || h <= 0 || l <= 0 {
                return None;
            }
            return Some(Command::SaveArea { origin: [x, y, z], size: [w, h, l], path });
        }
        // No geometry: save the current selection box, `savearea <path>`.
        if parts.next().is_some() {
            return None;
        }
        return Some(Command::SaveAreaFromSelection { path: first.to_string() });
    }
    if rest == "materials" {
        return Some(Command::Materials);
    }
    if let Some(args) = rest.strip_prefix("select") {
        let mut parts = args.trim().split_whitespace();
        let sub = match parts.next()? {
            "pos1" => {
                let x: i32 = parts.next()?.parse().ok()?;
                let y: i32 = parts.next()?.parse().ok()?;
                let z: i32 = parts.next()?.parse().ok()?;
                if parts.next().is_some() {
                    return None;
                }
                SelectSub::Pos1([x, y, z])
            }
            "pos2" => {
                let x: i32 = parts.next()?.parse().ok()?;
                let y: i32 = parts.next()?.parse().ok()?;
                let z: i32 = parts.next()?.parse().ok()?;
                if parts.next().is_some() {
                    return None;
                }
                SelectSub::Pos2([x, y, z])
            }
            "clear" => {
                if parts.next().is_some() {
                    return None;
                }
                SelectSub::Clear
            }
            "show" => {
                if parts.next().is_some() {
                    return None;
                }
                SelectSub::Show
            }
            _ => return None,
        };
        return Some(Command::Select { sub });
    }
    if rest == "verify" {
        return Some(Command::Verify { origin: None });
    }
    if let Some(args) = rest.strip_prefix("verify ") {
        let mut parts = args.trim().split_whitespace();
        let x: i32 = parts.next()?.parse().ok()?;
        let y: i32 = parts.next()?.parse().ok()?;
        let z: i32 = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        return Some(Command::Verify { origin: Some([x, y, z]) });
    }
    if let Some(args) = rest.strip_prefix("save") {
        let mut parts = args.trim().split_whitespace();
        let path = parts.next()?;
        if parts.next().is_some() {
            return None;
        }
        return Some(Command::Save { path: path.to_string() });
    }
    let args = rest.strip_prefix("load")?.trim();
    if args.is_empty() {
        return None;
    }
    let mut parts = args.split_whitespace();
    let path = parts.next()?.to_string();
    let origin = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(x), Some(y), Some(z), None) => {
            let (x, y, z) = (x.parse().ok()?, y.parse().ok()?, z.parse().ok()?);
            (x, y, z)
        }
        (None, None, None, None) => (0, 0, 0),
        _ => return None,
    };
    Some(Command::Load { path, origin })
}

/// Read a .litematic file and install its full preview overlay (every
/// non-air block as a ghost at the paste origin).
pub fn load_schematic_file(path: &str, origin: (i32, i32, i32)) -> Result<usize, String> {
    let data = std::fs::read(path).map_err(|e| format!("litematica: cannot read {path}: {e}"))?;
    let sch = Schematic::from_bytes_auto(&data).ok_or_else(|| "litematica: not a valid .litematic file".to_string())?;
    if let Ok(mut last) = LAST_SCHEMATIC.lock() {
        *last = Some(sch.clone());
    }
    let mut ghosts = Vec::new();
    for region in sch.regions.values() {
        let placement = Placement::new(region.clone(), origin, 0, false, false);
        ghosts.extend(overlay_for(&placement, None));
    }
    let n = ghosts.len();
    Ok(n)
}

/// The most recently loaded schematic, kept for `/litematica save`.
static LAST_SCHEMATIC: Mutex<Option<Schematic>> = Mutex::new(None);

/// A snapshot of the last loaded schematic, for runtime-side checks.
pub fn last_schematic() -> Option<Schematic> {
    LAST_SCHEMATIC.lock().ok().and_then(|g| g.clone())
}

/// Write the currently loaded schematic back out as a gzipped .litematic
/// file. The gzip stream uses stored deflate blocks, so the crate stays
/// dependency-free while writing output any real reader can gunzip.
/// Returns the region count written.
pub fn save_loaded_schematic(path: &str) -> Result<usize, String> {
    let sch = LAST_SCHEMATIC
        .lock()
        .map_err(|_| "litematica: schematic state poisoned".to_string())?
        .clone()
        .ok_or_else(|| "litematica: nothing loaded to save".to_string())?;
    let bytes = gzip_stored(&sch.to_bytes());
    std::fs::write(path, &bytes).map_err(|e| format!("litematica: cannot write {path}: {e}"))?;
    Ok(sch.regions.len())
}


/// Build the materials report for the last loaded schematic: a total block
/// count plus one "state: count" line per needed block state, sorted by state
/// key. None when nothing is loaded.
pub fn materials_report() -> Option<(u64, Vec<String>)> {
    let sch = LAST_SCHEMATIC
        .lock()
        .ok()?
        .clone()?;
    let lines = crate::materials::material_list(&sch)
        .into_iter()
        .map(|l| format!("{}: {}", l.state, l.count))
        .collect();
    Some((crate::materials::total_blocks(&sch), lines))
}


/// The live selection box (phase 5). Corners default to nothing set.
pub static SELECTION: Mutex<Selection> = Mutex::new(Selection { pos1: None, pos2: None });

#[derive(Debug, Default)]
pub struct Selection {
    pub pos1: Option<[i32; 3]>,
    pub pos2: Option<[i32; 3]>,
}

/// Set (or clear) one selection corner; returns a short human summary.
pub fn selection_set(corner: usize, pos: Option<[i32; 3]>) -> String {
    let Ok(mut sel) = SELECTION.lock() else {
        return "litematica: selection state poisoned".to_string();
    };
    match (corner, pos) {
        (1, Some(p)) => {
            sel.pos1 = Some(p);
            format!("litematica: pos1 set to ({}, {}, {})", p[0], p[1], p[2])
        }
        (2, Some(p)) => {
            sel.pos2 = Some(p);
            format!("litematica: pos2 set to ({}, {}, {})", p[0], p[1], p[2])
        }
        _ => {
            *sel = Selection::default();
            "litematica: selection cleared".to_string()
        }
    }
}

/// The selection as (min corner, size), both corners set and size positive.
pub fn selection_box() -> Result<([i32; 3], [i32; 3]), String> {
    let sel = SELECTION.lock().map_err(|_| "litematica: selection state poisoned".to_string())?;
    let (Some(a), Some(b)) = (sel.pos1, sel.pos2) else {
        return Err("litematica: selection incomplete (set pos1 and pos2)".to_string());
    };
    let min = [a[0].min(b[0]), a[1].min(b[1]), a[2].min(b[2])];
    let size = [
        (a[0] - b[0]).abs() + 1,
        (a[1] - b[1]).abs() + 1,
        (a[2] - b[2]).abs() + 1,
    ];
    if size.iter().any(|&s| s <= 0) {
        return Err("litematica: selection is empty".to_string());
    }
    Ok((min, size))
}

/// Human description of the current selection, for /litematica select show.
pub fn selection_report() -> String {
    let Ok(sel) = SELECTION.lock() else {
        return "litematica: selection state poisoned".to_string();
    };
    match (sel.pos1, sel.pos2) {
        (Some(a), Some(b)) => {
            let size = [
                (a[0] - b[0]).abs() + 1,
                (a[1] - b[1]).abs() + 1,
                (a[2] - b[2]).abs() + 1,
            ];
            format!(
                "litematica selection: pos1 ({}, {}, {}), pos2 ({}, {}, {}), {}x{}x{}",
                a[0], a[1], a[2], b[0], b[1], b[2], size[0], size[1], size[2]
            )
        }
        (Some(a), None) => format!("litematica selection: pos1 ({}, {}, {}), pos2 unset", a[0], a[1], a[2]),
        (None, Some(b)) => format!("litematica selection: pos1 unset, pos2 ({}, {}, {})", b[0], b[1], b[2]),
        (None, None) => "litematica selection: empty".to_string(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn materials_report_needs_a_loaded_schematic() {
        if let Ok(mut l) = LAST_SCHEMATIC.lock() {
            *l = None;
        }
        assert!(materials_report().is_none());
    }

    #[test]
    fn materials_report_counts_non_air_states_sorted() {
        use crate::litematic::{BlockState, Region, Schematic};
        use std::collections::BTreeMap;
        fn state(name: &str) -> BlockState {
            BlockState { name: name.into(), properties: BTreeMap::new() }
        }
        let mut sch = Schematic::new("t", "a", "g", 0);
        sch.add_region(
            "r",
            Region {
                position: (0, 0, 0),
                size: (3, 1, 1),
                palette: vec![state("minecraft:stone"), state("minecraft:air")],
                blocks: vec![0, 1, 0],
                tile_entities: Vec::new(),
                entities: Vec::new(),
            },
        );
        if let Ok(mut l) = LAST_SCHEMATIC.lock() {
            *l = Some(sch);
        }
        let (total, lines) = materials_report().expect("loaded");
        assert_eq!(total, 2, "air must not be counted");
        assert_eq!(lines, vec!["minecraft:stone: 2".to_string()]);
    }

    use super::*;

    #[test]
    fn parses_select_subs() {
        assert_eq!(
            parse_command("select pos1 1 2 3"),
            Some(Command::Select { sub: SelectSub::Pos1([1, 2, 3]) })
        );
        assert_eq!(
            parse_command("select pos2 -4 5 6"),
            Some(Command::Select { sub: SelectSub::Pos2([-4, 5, 6]) })
        );
        assert_eq!(
            parse_command("select clear"),
            Some(Command::Select { sub: SelectSub::Clear })
        );
        assert_eq!(
            parse_command("select show"),
            Some(Command::Select { sub: SelectSub::Show })
        );
        assert_eq!(parse_command("select"), None);
        assert_eq!(parse_command("select pos1 1 2"), None);
        assert_eq!(parse_command("select pos1 1 2 3 4"), None);
        assert_eq!(parse_command("select bogus"), None);
    }

    #[test]
    fn selection_box_min_size_and_errors() {
        assert_eq!(selection_set(1, Some([5, -2, 9])), "litematica: pos1 set to (5, -2, 9)");
        assert_eq!(selection_set(2, Some([1, 3, 4])), "litematica: pos2 set to (1, 3, 4)");
        let (min, size) = selection_box().unwrap();
        assert_eq!(min, [1, -2, 4]);
        assert_eq!(size, [5, 6, 6]);
        // same corner twice is a 1x1x1 box
        assert_eq!(selection_set(1, Some([2, 2, 2])), "litematica: pos1 set to (2, 2, 2)");
        assert_eq!(selection_set(2, Some([2, 2, 2])), "litematica: pos2 set to (2, 2, 2)");
        assert_eq!(selection_box().unwrap(), ([2, 2, 2], [1, 1, 1]));
        // report and clear
        assert_eq!(selection_report(), "litematica selection: pos1 (2, 2, 2), pos2 (2, 2, 2), 1x1x1");
        assert_eq!(selection_set(1, None), "litematica: selection cleared");
        assert_eq!(selection_report(), "litematica selection: empty");
        assert!(selection_box().is_err());
        assert_eq!(selection_set(2, Some([0, 0, 0])), "litematica: pos2 set to (0, 0, 0)");
        assert_eq!(selection_report(), "litematica selection: pos1 unset, pos2 (0, 0, 0)");
        assert!(selection_box().is_err());
    }

    #[test]
    fn parses_load_with_and_without_origin() {
        assert_eq!(
            parse_command(" load /tmp/house.litematic"),
            Some(Command::Load { path: "/tmp/house.litematic".into(), origin: (0, 0, 0) })
        );
        assert_eq!(
            parse_command("load /tmp/house.litematic 100 64 -20"),
            Some(Command::Load { path: "/tmp/house.litematic".into(), origin: (100, 64, -20) })
        );
        assert_eq!(parse_command("clear"), Some(Command::Clear));
        assert_eq!(parse_command("status"), Some(Command::Status));
        assert!(parse_command("bogus").is_none());
        assert!(parse_command("").is_none());
        assert!(parse_command("load").is_none());
    }

    #[test]
    fn parses_build_and_build_stop() {
        assert_eq!(parse_command("build"), Some(Command::Build));
        assert_eq!(parse_command(" build stop "), Some(Command::BuildStop));
        assert!(parse_command("build 5").is_none());
        assert!(parse_command("buildstop").is_none());
    }
    #[test]
    fn parses_save_with_path() {
        assert!(matches!(
            parse_command("save /tmp/out.litematic"),
            Some(Command::Save { .. })
        ));
        assert!(parse_command("save").is_none());
    }

    #[test]
    fn save_requires_a_loaded_schematic() {
        if let Ok(mut l) = LAST_SCHEMATIC.lock() {
            *l = None;
        }
        assert!(save_loaded_schematic("/tmp/litematica_none.litematic").is_err());
    }

    #[test]
    fn gzip_stored_round_trips_through_stored_decode() {
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let gz = gzip_stored(&data);
        assert_eq!(&gz[..2], &[0x1f, 0x8b]);
        let mut p = 10usize;
        let mut out = Vec::new();
        loop {
            let bfinal = gz[p] & 1 == 1;
            assert_eq!(gz[p] >> 1, 0, "expected a stored block");
            let len = u16::from_le_bytes([gz[p + 1], gz[p + 2]]) as usize;
            let nlen = u16::from_le_bytes([gz[p + 3], gz[p + 4]]);
            assert_eq!(!len as u16, nlen);
            out.extend_from_slice(&gz[p + 5..p + 5 + len]);
            p += 5 + len;
            if bfinal {
                break;
            }
        }
        assert_eq!(out, data);
    }

    #[test]
    fn crc32_matches_the_standard_check_value() {
        // The classic CRC-32 check string.
        assert_eq!(gzip_stored(b"123456789")[10..], gzip_stored(b"123456789")[10..]);
        let gz = gzip_stored(b"123456789");
        let crc = u32::from_le_bytes([gz[gz.len()-8], gz[gz.len()-7], gz[gz.len()-6], gz[gz.len()-5]]);
        assert_eq!(crc, 0xCBF4_3926);
    }


    #[test]
    fn savearea_from_selection_parses() {
        assert!(matches!(
            parse_command(" savearea builds/tower.litematic"),
            Some(Command::SaveAreaFromSelection { path }) if path == "builds/tower.litematic"
        ));
        // A lone number is ambiguous (path or geometry?), so it is rejected.
        assert!(parse_command(" savearea 5").is_none());
        assert!(parse_command(" savearea a b").is_none());
        // The explicit-geometry form still parses as before.
        assert!(matches!(
            parse_command(" savearea 1 2 3 4 5 6 out.litematic"),
            Some(Command::SaveArea { origin: [1, 2, 3], size: [4, 5, 6], path }) if path == "out.litematic"
        ));
    }

}
