//! The .litematic container: Metadata + Regions, ported 1:1 against the
//! format Litematica (maruohon, LGPL-3.0) writes. Format details verified
//! against litemapy (SmylerMC/litemapy), the reference implementation of the
//! format: palette bit width = max(ceil(log2(palette_len)), 2), bit-packed
//! continuously across i64 words, LSB-first within each word, block indices
//! ordered y-major (ind = y*|w*l| + z*|w| + x), region Size stored signed with
//! abs(dimension) - 1 encoded as the sign direction.

use crate::nbt::{self, Reader, Value};
use std::collections::BTreeMap;

/// One block state: an identifier plus block-state properties.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockState {
    pub name: String,
    pub properties: BTreeMap<String, String>,
}

impl BlockState {
    pub fn air() -> Self { Self { name: "minecraft:air".into(), properties: BTreeMap::new() } }

    pub fn to_nbt(&self) -> Value {
        let mut c = BTreeMap::new();
        c.insert("Name".into(), Value::String(self.name.clone()));
        if !self.properties.is_empty() {
            c.insert(
                "Properties".into(),
                Value::Compound(self.properties.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect()),
            );
        }
        Value::Compound(c)
    }

    pub fn from_nbt(v: &Value) -> Option<Self> {
        let Value::Compound(c) = v else { return None };
        let name = match c.get("Name") {
            Some(Value::String(s)) => s.clone(),
            _ => return None,
        };
        let mut properties = BTreeMap::new();
        if let Some(Value::Compound(props)) = c.get("Properties") {
            for (k, v) in props {
                if let Value::String(s) = v {
                    properties.insert(k.clone(), s.clone());
                }
            }
        }
        Some(Self { name, properties })
    }
}

/// The Litematica bit-packed state array: continuous LSB-first packing across
/// 64-bit words (no per-word alignment, unlike Sponge schematics).
pub struct PackedStates {
    words: Vec<u64>,
    nbits: u8,
    len: usize,
}

impl PackedStates {
    pub fn from_values(values: &[u32], nbits: u8) -> Self {
        let total_bits = values.len() as u64 * nbits as u64;
        let mut words = vec![0u64; total_bits.div_ceil(64) as usize];
        for (i, v) in values.iter().enumerate() {
            let start = i as u64 * nbits as u64;
            let w0 = (start / 64) as usize;
            let off = start % 64;
            words[w0] |= (*v as u64) << off;
            if off + nbits as u64 > 64 {
                words[w0 + 1] |= (*v as u64) >> (64 - off);
            }
        }
        Self { words, nbits, len: values.len() }
    }

    pub fn get(&self, index: usize) -> Option<u32> {
        if index >= self.len {
            return None;
        }
        let start = index as u64 * self.nbits as u64;
        let w0 = (start / 64) as usize;
        let off = (start % 64) as u32;
        let mask = (1u64 << self.nbits) - 1;
        if off + self.nbits as u32 <= 64 {
            Some(((self.words[w0] >> off) & mask) as u32)
        } else {
            let lo = self.words[w0] >> off;
            let hi = self.words[w0 + 1] << (64 - off);
            Some(((lo | hi) & mask) as u32)
        }
    }

    pub fn to_long_array(&self) -> Vec<i64> {
        self.words.iter().map(|w| *w as i64).collect()
    }

    pub fn from_long_array(longs: &[i64], len: usize, nbits: u8) -> Self {
        Self {
            words: longs.iter().map(|l| *l as u64).collect(),
            nbits,
            len,
        }
    }
}

/// The bit width a palette needs, per the format's rule.
pub fn nbits_for_palette(palette_len: usize) -> u8 {
    ((palette_len as f64).log2().ceil() as u8).max(2)
}

#[derive(Clone, Debug)]
pub struct Region {
    /// Region origin in world blocks (Position in the file).
    pub position: (i32, i32, i32),
    /// Signed dimensions; the sign encodes which way the region extends
    /// (toward + or -), abs() is the size.
    pub size: (i32, i32, i32),
    pub palette: Vec<BlockState>,
    /// Unpacked palette indices, x fastest, then z, then y.
    pub blocks: Vec<u32>,
    pub tile_entities: Vec<Value>,
    pub entities: Vec<Value>,
}

impl Region {
    pub fn dims(&self) -> (usize, usize, usize) {
        (self.size.0.unsigned_abs() as usize, self.size.1.unsigned_abs() as usize, self.size.2.unsigned_abs() as usize)
    }

    pub fn volume(&self) -> usize {
        let (w, h, l) = self.dims();
        w * h * l
    }

    /// The palette index at (x, y, z), region-local, x fastest then z then y.
    pub fn index_of(&self, x: usize, y: usize, z: usize) -> usize {
        let (w, _, l) = self.dims();
        y * (w * l) + z * w + x
    }

    pub fn get(&self, x: usize, y: usize, z: usize) -> &BlockState {
        &self.palette[self.blocks[self.index_of(x, y, z)] as usize]
    }

    pub fn set(&mut self, x: usize, y: usize, z: usize, state: u32) {
        let i = self.index_of(x, y, z);
        self.blocks[i] = state;
    }

    pub fn packed(&self) -> PackedStates {
        PackedStates::from_values(&self.blocks, nbits_for_palette(self.palette.len()))
    }

    pub fn to_nbt(&self, name: &str) -> (String, Value) {
        let mut c = BTreeMap::new();
        let mut pos = BTreeMap::new();
        pos.insert("x".into(), Value::Int(self.position.0));
        pos.insert("y".into(), Value::Int(self.position.1));
        pos.insert("z".into(), Value::Int(self.position.2));
        c.insert("Position".into(), Value::Compound(pos));
        let mut size = BTreeMap::new();
        size.insert("x".into(), Value::Int(self.size.0));
        size.insert("y".into(), Value::Int(self.size.1));
        size.insert("z".into(), Value::Int(self.size.2));
        c.insert("Size".into(), Value::Compound(size));
        c.insert("BlockStatePalette".into(), Value::List(self.palette.iter().map(|s| s.to_nbt()).collect()));
        let packed = self.packed();
        c.insert("BlockStates".into(), Value::LongArray(packed.to_long_array()));
        c.insert("TileEntities".into(), Value::List(self.tile_entities.clone()));
        c.insert("Entities".into(), Value::List(self.entities.clone()));
        (name.to_string(), Value::Compound(c))
    }

    pub fn from_nbt_plain(name: &str, v: &Value) -> Option<Self> {
        let Value::Compound(c) = v else { return None };
        let pos = compound_xyz(c.get("Position")?)?;
        let size = compound_xyz(c.get("Size")?)?;
        let Value::List(palette_nbt) = c.get("BlockStatePalette")? else { return None };
        let palette: Vec<BlockState> = palette_nbt.iter().filter_map(BlockState::from_nbt).collect();
        let Value::LongArray(longs) = c.get("BlockStates")? else { return None };
        let volume = size.0.unsigned_abs() as usize * size.1.unsigned_abs() as usize * size.2.unsigned_abs() as usize;
        let packed = PackedStates::from_long_array(longs, volume, nbits_for_palette(palette.len()));
        let blocks: Vec<u32> = (0..volume).map(|i| packed.get(i).unwrap_or(0)).collect();
        let tile_entities = match c.get("TileEntities") {
            Some(Value::List(l)) => l.clone(),
            _ => Vec::new(),
        };
        let entities = match c.get("Entities") {
            Some(Value::List(l)) => l.clone(),
            _ => Vec::new(),
        };
        Some(Self { position: pos, size, palette, blocks, tile_entities, entities })
    }
}

/// Reads an x/y/z int compound.
fn compound_xyz(v: &Value) -> Option<(i32, i32, i32)> {
    let Value::Compound(c) = v else { return None };
    let get = |k: &str| match c.get(k) {
        Some(Value::Int(i)) => Some(*i),
        _ => None,
    };
    Some((get("x")?, get("y")?, get("z")?))
}

#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub version: i32,
    pub sub_version: i32,
    pub minecraft_data_version: i32,
    pub generator: String,
    pub name: String,
    pub author: String,
    pub description: String,
    pub region_count: i32,
    pub total_blocks: i32,
    pub total_volume: i32,
    pub enclosing_size: (i32, i32, i32),
}

#[derive(Clone, Debug, Default)]
pub struct Schematic {
    pub metadata: Metadata,
    /// Regions keyed by name.
    pub regions: BTreeMap<String, Region>,
}

impl Schematic {
    pub fn new(name: &str, author: &str, generator: &str, mc_data_version: i32) -> Self {
        Self {
            metadata: Metadata { version: 5, sub_version: 0, minecraft_data_version: mc_data_version, generator: generator.into(), name: name.into(), author: author.into(), ..Default::default() },
            regions: BTreeMap::new(),
        }
    }

    pub fn add_region(&mut self, name: &str, region: Region) {
        self.regions.insert(name.into(), region);
    }

    /// Recompute the derived metadata fields after editing regions.
    pub fn recompute_metadata(&mut self) {
        self.metadata.region_count = self.regions.len() as i32;
        let mut total_blocks = 0usize;
        let mut total_volume = 0usize;
        let mut min = [i32::MAX; 3];
        let mut max = [i32::MIN; 3];
        for r in self.regions.values() {
            let (w, h, l) = r.dims();
            total_volume += w * h * l;
            total_blocks += r.blocks.iter().filter(|b| **b != 0).count();
            let (px, py, pz) = r.position;
            let (sx, sy, sz) = (r.size.0.signum(), r.size.1.signum(), r.size.2.signum());
            let corners = [
                (px, py, pz),
                (px + sx * r.size.0.unsigned_abs() as i32, py, pz),
                (px, py + sy * r.size.1.unsigned_abs() as i32, pz),
                (px, py, pz + sz * r.size.2.unsigned_abs() as i32),
                (px + sx * r.size.0.unsigned_abs() as i32, py + sy * r.size.1.unsigned_abs() as i32, pz),
                (px + sx * r.size.0.unsigned_abs() as i32, py, pz + sz * r.size.2.unsigned_abs() as i32),
                (px, py + sy * r.size.1.unsigned_abs() as i32, pz + sz * r.size.2.unsigned_abs() as i32),
                (px + sx * r.size.0.unsigned_abs() as i32, py + sy * r.size.1.unsigned_abs() as i32, pz + sz * r.size.2.unsigned_abs() as i32),
            ];
            for c in corners {
                for a in 0..3 {
                    min[a] = min[a].min([c.0, c.1, c.2][a]);
                    max[a] = max[a].max([c.0, c.1, c.2][a]);
                }
            }
        }
        self.metadata.total_blocks = total_blocks as i32;
        self.metadata.total_volume = total_volume as i32;
        if self.regions.is_empty() {
            self.metadata.enclosing_size = (0, 0, 0);
        } else {
            self.metadata.enclosing_size = (max[0] - min[0], max[1] - min[1], max[2] - min[2]);
        }
    }

    /// Serialize to uncompressed .litematic bytes (the caller gzips).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut inner = BTreeMap::new();
        let mut m = BTreeMap::new();
        m.insert("Version".into(), Value::Int(self.metadata.version));
        m.insert("SubVersion".into(), Value::Int(self.metadata.sub_version));
        m.insert("MinecraftDataVersion".into(), Value::Int(self.metadata.minecraft_data_version));
        m.insert("Generator".into(), Value::String(self.metadata.generator.clone()));
        m.insert("Name".into(), Value::String(self.metadata.name.clone()));
        m.insert("Author".into(), Value::String(self.metadata.author.clone()));
        if !self.metadata.description.is_empty() {
            m.insert("Description".into(), Value::String(self.metadata.description.clone()));
        }
        m.insert("RegionCount".into(), Value::Int(self.metadata.region_count));
        m.insert("TotalBlocks".into(), Value::Int(self.metadata.total_blocks));
        m.insert("TotalVolume".into(), Value::Int(self.metadata.total_volume));
        let mut enc = BTreeMap::new();
        enc.insert("x".into(), Value::Int(self.metadata.enclosing_size.0));
        enc.insert("y".into(), Value::Int(self.metadata.enclosing_size.1));
        enc.insert("z".into(), Value::Int(self.metadata.enclosing_size.2));
        m.insert("EnclosingSize".into(), Value::Compound(enc));
        inner.insert("Metadata".into(), Value::Compound(m));
        let mut regions = BTreeMap::new();
        for (name, r) in &self.regions {
            let (n, v) = r.to_nbt(name);
            regions.insert(n, v);
        }
        inner.insert("Regions".into(), Value::Compound(regions));
        let mut root = BTreeMap::new();
        root.insert("".into(), Value::Compound(inner));
        nbt::write_root(&root)
    }

    /// Parse uncompressed .litematic bytes (the caller gunzips).
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        let root = Reader::new(data).root()?;
        let Value::Compound(inner) = root.get("")? else { return None };
        let Value::Compound(m) = inner.get("Metadata")? else { return None };
        let int_at = |c: &BTreeMap<String, Value>, k: &str| match c.get(k) {
            Some(Value::Int(i)) => Some(*i),
            _ => None,
        };
        let str_at = |c: &BTreeMap<String, Value>, k: &str| match c.get(k) {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        let mut metadata = Metadata {
            version: int_at(m, "Version").unwrap_or(5),
            sub_version: int_at(m, "SubVersion").unwrap_or(0),
            minecraft_data_version: int_at(m, "MinecraftDataVersion").unwrap_or(0),
            generator: str_at(m, "Generator"),
            name: str_at(m, "Name"),
            author: str_at(m, "Author"),
            description: str_at(m, "Description"),
            region_count: int_at(m, "RegionCount").unwrap_or(0),
            total_blocks: int_at(m, "TotalBlocks").unwrap_or(0),
            total_volume: int_at(m, "TotalVolume").unwrap_or(0),
            enclosing_size: (0, 0, 0),
        };
        if let Some(Value::Compound(enc)) = m.get("EnclosingSize") {
            metadata.enclosing_size = (
                int_at(enc, "x").unwrap_or(0),
                int_at(enc, "y").unwrap_or(0),
                int_at(enc, "z").unwrap_or(0),
            );
        }
        let Value::Compound(regions_nbt) = inner.get("Regions")? else { return None };
        let mut regions = BTreeMap::new();
        for (name, v) in regions_nbt {
            regions.insert(name.clone(), Region::from_nbt_plain(name, v)?);
        }
        Some(Self { metadata, regions })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_packing_matches_known_values() {
        // 3-bit packing of [5, 2, 7, 0] -> bit stream 101 010 111 000
        let p = PackedStates::from_values(&[5, 2, 7, 0], 3);
        assert_eq!(p.words[0] & 0xFFF, 469, "LSB-first: 5 | 2<<3 | 7<<6");
        for (i, want) in [5u32, 2, 7, 0].iter().enumerate() {
            assert_eq!(p.get(i), Some(*want));
        }
    }

    #[test]
    fn values_spanning_word_boundaries_round_trip() {
        // 7-bit values: 9 values * 7 bits = 63, so the 10th crosses into word 1.
        let vals: Vec<u32> = (0..40).map(|i| (i * 11) % 128).collect();
        let p = PackedStates::from_values(&vals, 7);
        assert_eq!(p.words.len(), 5, "40*7=280 bits -> 5 words");
        for (i, want) in vals.iter().enumerate() {
            assert_eq!(p.get(i), Some(*want), "index {i}");
        }
    }

    #[test]
    fn nbits_rule_is_max_ceil_log2_floor_2() {
        assert_eq!(nbits_for_palette(1), 2);
        assert_eq!(nbits_for_palette(4), 2);
        assert_eq!(nbits_for_palette(5), 3);
        assert_eq!(nbits_for_palette(8), 3);
        assert_eq!(nbits_for_palette(9), 4);
    }

    fn region2x2x2() -> Region {
        let mut r = Region {
            position: (10, 64, -5),
            size: (2, 2, 2),
            palette: vec![BlockState::air(), BlockState { name: "minecraft:stone".into(), properties: BTreeMap::new() }],
            blocks: vec![0; 8],
            tile_entities: Vec::new(),
            entities: Vec::new(),
        };
        r.set(1, 0, 1, 1);
        r
    }

    #[test]
    fn index_order_is_x_fastest_then_z_then_y() {
        let r = region2x2x2();
        // set(1, 0, 1) is index y=0: z*w+x = 1*2+1 = 3
        assert_eq!(r.index_of(1, 0, 1), 3);
        assert_eq!(r.blocks[3], 1);
        assert_eq!(r.get(1, 0, 1).name, "minecraft:stone");
    }

    #[test]
    fn litematic_round_trip() {
        let mut s = Schematic::new("test", "sparticle", "litematica-port", 3700);
        s.add_region("one", region2x2x2());
        let mut two = region2x2x2();
        two.size = (-2, 2, 2); // negative-direction extension
        two.position = (0, 0, 0);
        two.palette.push(BlockState { name: "minecraft:oak_log".into(), properties: [("axis".into(), "y".into())].into_iter().collect() });
        two.set(0, 1, 0, 2);
        s.add_region("two", two);
        s.recompute_metadata();

        let bytes = s.to_bytes();
        let back = Schematic::from_bytes(&bytes).unwrap();
        assert_eq!(back.metadata.name, "test");
        assert_eq!(back.metadata.region_count, 2);
        assert_eq!(back.regions.len(), 2);
        let one = &back.regions["one"];
        assert_eq!(one.position, (10, 64, -5));
        assert_eq!(one.get(1, 0, 1).name, "minecraft:stone");
        assert_eq!(one.get(0, 0, 0).name, "minecraft:air");
        let two = &back.regions["two"];
        assert_eq!(two.size.0, -2, "the negative-direction size survives the round trip");
        assert_eq!(two.get(0, 1, 0).name, "minecraft:oak_log");
        assert_eq!(two.palette[2].properties.get("axis").map(String::as_str), Some("y"));
        assert_eq!(back.metadata.total_blocks, 3, "region one has 1, region two has 2 nonzero blocks");
    }

    #[test]
    fn metadata_derived_fields() {
        let mut s = Schematic::new("m", "a", "g", 3700);
        s.add_region("r", region2x2x2()); // position (10,64,-5), size (2,2,2)
        s.recompute_metadata();
        assert_eq!(s.metadata.region_count, 1);
        assert_eq!(s.metadata.total_volume, 8);
        assert_eq!(s.metadata.total_blocks, 1);
        assert_eq!(s.metadata.enclosing_size, (2, 2, 2));
    }

    #[test]
    fn long_array_sign_survives() {
        let p = PackedStates::from_values(&[3, 3, 3, 3], 2);
        let longs = p.to_long_array();
        let p = PackedStates::from_values(&[3u32; 32], 2); // 64 bits -> word 0 all ones
        let longs = p.to_long_array();
        assert_eq!(longs[0], -1i64, "a full word must read back as a negative i64, like the Java side writes");
        let back = PackedStates::from_long_array(&longs, 32, 2);
        assert_eq!(back.get(31), Some(3));
    }
}
