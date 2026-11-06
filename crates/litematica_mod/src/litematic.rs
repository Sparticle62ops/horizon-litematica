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


/// CRC-32 (IEEE), shared by the gzip writer and reader.
pub(crate) fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, e) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *e = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// gzip writer using stored (uncompressed) deflate blocks, so the crate
/// stays dependency-free while producing a valid gzip stream.
pub(crate) fn gzip_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff];
    if data.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
    } else {
        let mut chunks = data.chunks(65_535).peekable();
        while let Some(chunk) = chunks.next() {
            let last = chunks.peek().is_none();
            out.push(if last { 0x01 } else { 0x00 });
            let n = chunk.len() as u16;
            out.extend_from_slice(&n.to_le_bytes());
            out.extend_from_slice(&(!n).to_le_bytes());
            out.extend_from_slice(chunk);
        }
    }
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

/// LSB-first bit reader over a byte slice.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
    cnt: u32,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0, bit: 0, cnt: 0 }
    }

    /// Read `need` bits (LSB first); `need` must be 0..=25.
    fn bits(&mut self, need: u32) -> Option<u32> {
        while self.cnt < need {
            let b = *self.data.get(self.pos)? as u32;
            self.pos += 1;
            self.bit |= b << self.cnt;
            self.cnt += 8;
        }
        let out = self.bit & ((1u32 << need).wrapping_sub(1));
        self.bit >>= need;
        self.cnt -= need;
        Some(out)
    }
}

/// Canonical Huffman decoding table built from code lengths.
struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Option<Huffman> {
        let mut count = [0u16; 16];
        for &l in lengths {
            if l as usize >= 16 {
                return None;
            }
            if l != 0 {
                count[l as usize] += 1;
            }
        }
        // Reject over-subscribed codes; incomplete ones are tolerated and
        // fail later if a missing symbol is actually decoded.
        let mut left = 1i32;
        for len in 1..16 {
            left <<= 1;
            left -= count[len] as i32;
            if left < 0 {
                return None;
            }
        }
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.iter().filter(|&&l| l != 0).count()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l as usize] as usize] = sym as u16;
                offs[l as usize] += 1;
            }
        }
        Some(Huffman { count, symbol })
    }

    fn decode(&self, r: &mut BitReader) -> Option<u16> {
        let mut code = 0i32;
        let mut first = 0i32;
        let mut index = 0i32;
        for len in 1..16 {
            code |= r.bits(1)? as i32;
            let count = self.count[len] as i32;
            if code - count < first {
                return self.symbol.get((index + (code - first)) as usize).copied();
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        None
    }
}

const LBASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEXT: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DBASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DEXT: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// Inflate raw deflate data (stored, fixed and dynamic Huffman blocks),
/// dependency-free. Returns None on malformed input.
fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let mut r = BitReader::new(data);
    let mut out: Vec<u8> = Vec::new();
    loop {
        let bfinal = r.bits(1)?;
        let btype = r.bits(2)?;
        match btype {
            0 => {
                // Stored: discard the rest of the current byte.
                r.bit = 0;
                r.cnt = 0;
                if r.pos + 4 > data.len() {
                    return None;
                }
                let len = u16::from_le_bytes([data[r.pos], data[r.pos + 1]]) as usize;
                let nlen = u16::from_le_bytes([data[r.pos + 2], data[r.pos + 3]]);
                if nlen != !(len as u16) {
                    return None;
                }
                r.pos += 4;
                if r.pos + len > data.len() {
                    return None;
                }
                out.extend_from_slice(&data[r.pos..r.pos + len]);
                r.pos += len;
            }
            1 => {
                let mut lit_lengths = [0u8; 288];
                for (i, l) in lit_lengths.iter_mut().enumerate() {
                    *l = if i < 144 { 8 } else if i < 256 { 9 } else if i < 280 { 7 } else { 8 };
                }
                let dist_lengths = [5u8; 30];
                let lit = Huffman::new(&lit_lengths)?;
                let dist = Huffman::new(&dist_lengths)?;
                inflate_block(&mut r, &lit, &dist, &mut out)?;
            }
            2 => {
                let hlit = r.bits(5)? as usize + 257;
                let hdist = r.bits(5)? as usize + 1;
                let hclen = r.bits(4)? as usize + 4;
                const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                let mut cl_lengths = [0u8; 19];
                for i in 0..hclen {
                    cl_lengths[ORDER[i]] = r.bits(3)? as u8;
                }
                let cl = Huffman::new(&cl_lengths)?;
                let mut lengths = vec![0u8; hlit + hdist];
                let mut i = 0usize;
                while i < lengths.len() {
                    let sym = cl.decode(&mut r)?;
                    match sym {
                        0..=15 => {
                            lengths[i] = sym as u8;
                            i += 1;
                        }
                        16 => {
                            if i == 0 {
                                return None;
                            }
                            let prev = lengths[i - 1];
                            let rep = 3 + r.bits(2)? as usize;
                            if i + rep > lengths.len() {
                                return None;
                            }
                            for _ in 0..rep {
                                lengths[i] = prev;
                                i += 1;
                            }
                        }
                        17 => {
                            let rep = 3 + r.bits(3)? as usize;
                            if i + rep > lengths.len() {
                                return None;
                            }
                            i += rep;
                        }
                        18 => {
                            let rep = 11 + r.bits(7)? as usize;
                            if i + rep > lengths.len() {
                                return None;
                            }
                            i += rep;
                        }
                        _ => return None,
                    }
                }
                if lengths.get(256).copied().unwrap_or(0) == 0 {
                    return None;
                }
                let lit = Huffman::new(&lengths[..hlit])?;
                let dist = Huffman::new(&lengths[hlit..])?;
                inflate_block(&mut r, &lit, &dist, &mut out)?;
            }
            _ => return None,
        }
        if bfinal == 1 {
            break;
        }
    }
    Some(out)
}

fn inflate_block(r: &mut BitReader, lit: &Huffman, dist: &Huffman, out: &mut Vec<u8>) -> Option<()> {
    loop {
        let sym = lit.decode(r)?;
        match sym {
            0..=255 => out.push(sym as u8),
            256 => return Some(()),
            257..=285 => {
                let i = sym as usize - 257;
                let length = LBASE[i] as usize + r.bits(LEXT[i] as u32)? as usize;
                let dsym = dist.decode(r)? as usize;
                if dsym >= 30 {
                    return None;
                }
                let distance = DBASE[dsym] as usize + r.bits(DEXT[dsym] as u32)? as usize;
                if distance > out.len() {
                    return None;
                }
                let start = out.len() - distance;
                for k in 0..length {
                    let b = out[start + k];
                    out.push(b);
                }
            }
            _ => return None,
        }
    }
}

/// gunzip a single gzip member (header flags, CRC-32 and length verified).
pub fn gunzip(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 18 || data[0] != 0x1f || data[1] != 0x8b || data[2] != 8 {
        return None;
    }
    let flg = data[3];
    let mut p = 10usize;
    if flg & 0x04 != 0 {
        if p + 2 > data.len() {
            return None;
        }
        let xlen = u16::from_le_bytes([data[p], data[p + 1]]) as usize;
        p += 2 + xlen;
    }
    if flg & 0x08 != 0 {
        while p < data.len() && data[p] != 0 {
            p += 1;
        }
        p += 1;
    }
    if flg & 0x10 != 0 {
        while p < data.len() && data[p] != 0 {
            p += 1;
        }
        p += 1;
    }
    if flg & 0x02 != 0 {
        p += 2;
    }
    if p + 8 > data.len() {
        return None;
    }
    let out = inflate(&data[p..data.len() - 8])?;
    let crc = u32::from_le_bytes([
        data[data.len() - 8],
        data[data.len() - 7],
        data[data.len() - 6],
        data[data.len() - 5],
    ]);
    let isize_ = u32::from_le_bytes([
        data[data.len() - 4],
        data[data.len() - 3],
        data[data.len() - 2],
        data[data.len() - 1],
    ]);
    if crc32(&out) != crc || (out.len() as u32) != isize_ {
        return None;
    }
    Some(out)
}


impl Schematic {
    /// Parse .litematic bytes that may be raw NBT or gzipped (the
    /// real-world format wraps the NBT in gzip).
    pub fn from_bytes_auto(data: &[u8]) -> Option<Self> {
        if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
            let raw = gunzip(data)?;
            return Self::from_bytes(&raw);
        }
        Self::from_bytes(data)
    }
}

const VECTOR_1_GZ: &str = "1f8b0800000000000003cb48cdc9c95728cf2fca4951c818016c00bce00908f0000000";
const VECTOR_2_GZ: &str = "1f8b0800000000000203edcadb1182301400d1566e05f4a4100451a221c147f53a56e1c7f9dcd953a714f736f74b1c4b7eac31e6679cdbf5b645de5389fadd97c3fb15433e75bf82611886611886611886611886611886611886611886611886611886611886611886611886611886611886611886611886611886ff127f00127f7f1c50460000";
const VECTOR_3_GZ: &str = "1f8b0808c221ed6a02ff746573742e6c6974656d6174696300cb48cdc9c95728cf2fca4951c818016c00bce00908f0000000";
const VECTOR_1_IN: &[u8] = b"hello world hello world hello world hello world hello world hello world hello world hello world hello world hello world hello world hello world hello world hello world hello world hello world hello world hello world hello world hello world ";
const VECTOR_2_IN: &[u8] = b"the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. the quick brown fox jumps over the lazy dog. ";

/// Parse a state key like "minecraft:oak_planks[facing=north]" into a
/// BlockState. Bare names carry no properties.
pub fn parse_state_key(key: &str) -> Option<BlockState> {
    let key = key.trim();
    let (name, props) = match key.find('[') {
        Some(i) => {
            if !key.ends_with(']') {
                return None;
            }
            (&key[..i], &key[i + 1..key.len() - 1])
        }
        None => (key, ""),
    };
    if name.is_empty() {
        return None;
    }
    let mut properties = BTreeMap::new();
    if !props.is_empty() {
        for pair in props.split(',') {
            let (k, v) = pair.split_once('=')?;
            properties.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    Some(BlockState { name: name.to_string(), properties })
}

/// Build a Schematic from a box of state keys, x fastest, then z, then y.
/// `None` entries become air. Returns None for non-positive sizes, a wrong
/// key count, an unknown key, or a volume over 8,388,608 blocks (a fixed
/// cap that keeps captures bounded).
pub fn schematic_from_states(
    name: &str,
    origin: (i32, i32, i32),
    size: (i32, i32, i32),
    keys: &[Option<String>],
) -> Option<Schematic> {
    let (w, h, l) = size;
    if w <= 0 || h <= 0 || l <= 0 {
        return None;
    }
    let volume = w as i64 * h as i64 * l as i64;
    if volume > 8_388_608 || keys.len() as i64 != volume {
        return None;
    }
    let mut palette: Vec<BlockState> = vec![BlockState::air()];
    let mut seen: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut blocks: Vec<u32> = Vec::with_capacity(volume as usize);
    for key in keys {
        let Some(key) = key else {
            blocks.push(0);
            continue;
        };
        let idx = if let Some(&i) = seen.get(key) {
            i
        } else {
            let state = parse_state_key(key)?;
            palette.push(state);
            let i = (palette.len() - 1) as u32;
            seen.insert(key.clone(), i);
            i
        };
        blocks.push(idx);
    }
    let mut schematic = Schematic::new(name, "sparticle", "Horizon Litematica Mod", 4440);
    schematic.add_region(
        name,
        Region {
            position: origin,
            size,
            palette,
            blocks,
            tile_entities: Vec::new(),
            entities: Vec::new(),
        },
    );
    Some(schematic)
}

/// Write any schematic out as a gzipped .litematic file.
pub fn save_schematic(path: &str, schematic: &Schematic) -> Result<usize, String> {
    let bytes = gzip_stored(&schematic.to_bytes());
    std::fs::write(path, &bytes).map_err(|e| format!("litematica: cannot write {path}: {e}"))?;
    Ok(schematic.regions.len())
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
    #[test]
    fn gunzip_round_trips_the_stored_writer() {
        for size in [0usize, 1, 100, 65_535, 65_536, 131_073] {
            let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            let gz = gzip_stored(&data);
            assert_eq!(gunzip(&gz).as_deref(), Some(data.as_slice()), "size {size}");
        }
    }

    #[test]
    fn schematic_round_trips_through_gzip_save_and_load() {
        let mut sch = Schematic::new("rt", "tester", "litematica_port", 0);
        let stone = BlockState { name: "minecraft:stone".into(), properties: BTreeMap::new() };
        let region = Region {
            position: (0, 0, 0),
            size: (2, 1, 2),
            palette: vec![BlockState::air(), stone],
            blocks: vec![1, 0, 0, 1],
            tile_entities: vec![],
            entities: vec![],
        };
        sch.add_region("main", region);
        sch.recompute_metadata();
        let gz = gzip_stored(&sch.to_bytes());
        let back = Schematic::from_bytes_auto(&gz).expect("gzip round-trip");
        assert_eq!(back.regions.len(), 1);
        assert_eq!(back.metadata.total_blocks, 2);
        assert_eq!(back.regions["main"].blocks, vec![1u32, 0, 0, 1]);
        assert_eq!(back.regions["main"].palette[1].name, "minecraft:stone");
    }

    #[test]
    fn gunzip_reads_a_real_deflate_stream() {
        let gz1 = hex_to_bytes(VECTOR_1_GZ);
        assert_eq!(gunzip(&gz1).as_deref(), Some(VECTOR_1_IN));
        let gz2 = hex_to_bytes(VECTOR_2_GZ);
        assert_eq!(gunzip(&gz2).as_deref(), Some(VECTOR_2_IN));
        let gz3 = hex_to_bytes(VECTOR_3_GZ);
        assert_eq!(gunzip(&gz3).as_deref(), Some(VECTOR_1_IN));
    }

    fn hex_to_bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }
}

#[cfg(test)]
mod savearea_tests {
    use super::*;

    #[test]
    fn parse_state_key_parses_names_and_properties() {
        let s = parse_state_key("minecraft:oak_planks[facing=north,waterlogged=false]").unwrap();
        assert_eq!(s.name, "minecraft:oak_planks");
        assert_eq!(s.properties.get("facing").map(String::as_str), Some("north"));
        assert_eq!(s.properties.get("waterlogged").map(String::as_str), Some("false"));
        let plain = parse_state_key("minecraft:stone").unwrap();
        assert_eq!(plain.name, "minecraft:stone");
        assert!(plain.properties.is_empty());
        assert!(parse_state_key("minecraft:oops[broken").is_none());
        assert!(parse_state_key("").is_none());
    }

    #[test]
    fn from_states_dedupes_palette_and_orders_x_fastest() {
        let keys: Vec<Option<String>> = vec![
            Some("minecraft:stone".into()),
            None,
            Some("minecraft:stone".into()),
            Some("minecraft:dirt".into()),
        ];
        let schem = schematic_from_states("t", (1, 2, 3), (2, 2, 1), &keys).unwrap();
        let region = schem.regions.get("t").unwrap();
        assert_eq!(region.position, (1, 2, 3));
        assert_eq!(region.size, (2, 2, 1));
        assert_eq!(region.palette.len(), 3);
        assert_eq!(region.blocks, vec![1, 0, 1, 2]);
    }

    #[test]
    fn from_states_rejects_bad_sizes_and_round_trips() {
        assert!(schematic_from_states("bad", (0, 0, 0), (0, 1, 1), &[]).is_none());
        assert!(
            schematic_from_states("bad", (0, 0, 0), (2, 1, 1), &[Some("minecraft:stone".into())])
                .is_none()
        );
        let keys: Vec<Option<String>> = vec![
            Some("minecraft:stone".into()),
            None,
            Some("minecraft:oak_planks[facing=north]".into()),
            Some("minecraft:dirt".into()),
        ];
        let schem = schematic_from_states("r", (5, -3, 7), (2, 2, 1), &keys).unwrap();
        let bytes = schem.to_bytes();
        let loaded = Schematic::from_bytes_auto(&bytes).unwrap();
        let region = loaded.regions.get("r").unwrap();
        assert_eq!(region.position, (5, -3, 7));
        let i = region.index_of(1, 1, 0);
        assert_eq!(
            region.palette[region.blocks[i] as usize].name,
            "minecraft:dirt"
        );
    }
}

#[cfg(test)]
mod verify_wiring_tests {
    use super::*;

    #[test]
    fn verify_catches_a_wrong_block() {
        let want: Vec<Option<String>> = vec![
            Some("minecraft:stone".into()),
            Some("minecraft:dirt".into()),
            None,
            Some("minecraft:oak_planks[facing=north]".into()),
        ];
        let mut got = want.clone();
        got[1] = Some("minecraft:cobblestone".into());
        let a = schematic_from_states("v", (0, 0, 0), (2, 2, 1), &want).unwrap();
        let b = schematic_from_states("v", (0, 0, 0), (2, 2, 1), &got).unwrap();
        let ra = a.regions.get("v").unwrap();
        let rb = b.regions.get("v").unwrap();
        let ms = crate::verify::verify(rb, ra).unwrap();
        assert_eq!(ms.len(), 1);
        assert_eq!(ms[0].pos, (1, 0, 0));
        assert_eq!(ms[0].expected, "minecraft:dirt");
        assert_eq!(ms[0].found, "minecraft:cobblestone");
    }
}
