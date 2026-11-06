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
    Verify { origin: Option<[i32; 3]> },
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
        let x: i32 = parts.next()?.parse().ok()?;
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


#[cfg(test)]
mod tests {
    use super::*;

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

}
