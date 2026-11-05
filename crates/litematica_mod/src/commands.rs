//! The /litematica command surface, kept in the bundle crate so its pure
//! parts are unit-tested by the crate itself (runtime.rs calls into these).

use crate::litematic::Schematic;
use crate::overlay::overlay_for;
use crate::placements::Placement;

/// One parsed /litematica command.
#[derive(Debug, PartialEq)]
pub enum Command {
    Clear,
    Status,
    Load { path: String, origin: (i32, i32, i32) },
}

/// Parse a /litematica command body: "load <path> [x y z]", "clear",
/// "status". Returns None for anything else (the command is not ours).
pub fn parse_command(rest: &str) -> Option<Command> {
    let rest = rest.trim();
    if rest == "clear" {
        return Some(Command::Clear);
    }
    if rest == "status" {
        return Some(Command::Status);
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
    let sch = Schematic::from_bytes(&data).ok_or_else(|| "litematica: not a valid .litematic file".to_string())?;
    let mut ghosts = Vec::new();
    for region in sch.regions.values() {
        let placement = Placement::new(region.clone(), origin, 0, false, false);
        ghosts.extend(overlay_for(&placement, None));
    }
    let n = ghosts.len();
    Ok(n)
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
}
