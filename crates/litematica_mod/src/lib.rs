//! .litematic (NBT) reader/writer, ported from maruohon/litematica (LGPL-3.0).
//! Format details verified against litemapy, the reference implementation.
//! litematica runtime for Horizon (phase 1): the schematic data model and the
pub mod litematic;
pub mod materials;
pub mod nbt;
pub mod commands;
pub mod overlay;
pub mod placements;
pub mod printer;
pub mod verify;
