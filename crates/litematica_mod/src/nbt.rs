//! Minimal big-endian NBT reader/writer covering the tags the .litematic
//! format uses. Uncompressed, gzipped, and zlibped payloads are all handled by
//! the caller (the format wraps these bytes in gzip).

use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(Vec<Value>),
    Compound(BTreeMap<String, Value>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Value {
    pub fn id(&self) -> u8 {
        match self {
            Value::Byte(_) => 1,
            Value::Short(_) => 2,
            Value::Int(_) => 3,
            Value::Long(_) => 4,
            Value::Float(_) => 5,
            Value::Double(_) => 6,
            Value::ByteArray(_) => 7,
            Value::String(_) => 8,
            Value::List(_) => 9,
            Value::Compound(_) => 10,
            Value::IntArray(_) => 11,
            Value::LongArray(_) => 12,
        }
    }
}

pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self { Self { data, pos: 0 } }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let out = self.data.get(self.pos..self.pos + n)?;
        self.pos += n;
        Some(out)
    }

    fn u16(&mut self) -> Option<u16> {
        let b = self.take(2)?;
        Some(u16::from_be_bytes([b[0], b[1]]))
    }
    fn i16(&mut self) -> Option<i16> { Some(self.u16()? as i16) }
    fn i32(&mut self) -> Option<i32> {
        let b = self.take(4)?;
        Some(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn i64(&mut self) -> Option<i64> {
        let b = self.take(8)?;
        Some(i64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// One payload by tag id, with the payload's own name already read.
    pub fn payload(&mut self, tag: u8) -> Option<Value> {
        match tag {
            1 => Some(Value::Byte(self.take(1)?[0] as i8)),
            2 => Some(Value::Short(self.i16()?)),
            3 => Some(Value::Int(self.i32()?)),
            4 => Some(Value::Long(self.i64()?)),
            5 => {
                let b = self.take(4)?;
                Some(Value::Float(f32::from_be_bytes([b[0], b[1], b[2], b[3]])))
            }
            6 => {
                let b = self.take(8)?;
                Some(Value::Double(f64::from_be_bytes([
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
                ])))
            }
            7 => {
                let n = self.i32()? as usize;
                Some(Value::ByteArray(self.take(n)?.to_vec()))
            }
            8 => {
                let n = self.u16()? as usize;
                let b = self.take(n)?;
                Some(Value::String(String::from_utf8_lossy(b).into_owned()))
            }
            9 => {
                let inner = *self.take(1)?.first()?;
                let n = self.i32()? as usize;
                let mut out = Vec::with_capacity(n);
                for _ in 0..n {
                    out.push(self.payload(inner)?);
                }
                Some(Value::List(out))
            }
            10 => Some(Value::Compound(self.compound_body()?)),
            11 => {
                let n = self.i32()? as usize;
                let mut out = Vec::with_capacity(n);
                for _ in 0..n {
                    out.push(self.i32()?);
                }
                Some(Value::IntArray(out))
            }
            12 => {
                let n = self.i32()? as usize;
                let mut out = Vec::with_capacity(n);
                for _ in 0..n {
                    out.push(self.i64()?);
                }
                Some(Value::LongArray(out))
            }
            _ => None,
        }
    }

    /// A compound's entries (without the leading type byte or trailing End).
    pub fn compound_body(&mut self) -> Option<BTreeMap<String, Value>> {
        let mut map = BTreeMap::new();
        loop {
            let tag = *self.take(1)?.first()?;
            if tag == 0 {
                return Some(map);
            }
            let n = self.u16()? as usize;
            let name = String::from_utf8_lossy(self.take(n)?).into_owned();
            map.insert(name, self.payload(tag)?);
        }
    }

    /// The whole file: a root compound.
    pub fn root(&mut self) -> Option<BTreeMap<String, Value>> {
        if *self.take(1)?.first()? != 10 {
            return None;
        }
        let n = self.u16()? as usize;
        String::from_utf8_lossy(self.take(n)?); // root name
        self.compound_body()
    }
}

fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(bytes);
}

/// Write one payload by tag id, name already written by the caller.
pub fn write_payload(out: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Byte(x) => put(out, &[*x as u8]),
        Value::Short(x) => put(out, &x.to_be_bytes()),
        Value::Int(x) => put(out, &x.to_be_bytes()),
        Value::Long(x) => put(out, &x.to_be_bytes()),
        Value::Float(x) => put(out, &x.to_be_bytes()),
        Value::Double(x) => put(out, &x.to_be_bytes()),
        Value::ByteArray(b) => {
            put(out, &(b.len() as i32).to_be_bytes());
            put(out, b);
        }
        Value::String(s) => {
            put(out, &(s.len() as u16).to_be_bytes());
            put(out, s.as_bytes());
        }
        Value::List(items) => {
            let inner = items.first().map(Value::id).unwrap_or(0);
            put(out, &[inner]);
            put(out, &(items.len() as i32).to_be_bytes());
            for i in items {
                write_payload(out, i);
            }
        }
        Value::Compound(map) => write_compound_body(out, map),
        Value::IntArray(xs) => {
            put(out, &(xs.len() as i32).to_be_bytes());
            for x in xs {
                put(out, &x.to_be_bytes());
            }
        }
        Value::LongArray(xs) => {
            put(out, &(xs.len() as i32).to_be_bytes());
            for x in xs {
                put(out, &x.to_be_bytes());
            }
        }
    }
}

pub fn write_compound_body(out: &mut Vec<u8>, map: &BTreeMap<String, Value>) {
    for (name, v) in map {
        put(out, &[v.id()]);
        put(out, &(name.len() as u16).to_be_bytes());
        put(out, name.as_bytes());
        write_payload(out, v);
    }
    put(out, &[0]);
}

/// Write a whole root compound document (empty root name, NBT file convention).
pub fn write_root(map: &BTreeMap<String, Value>) -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, &[10]);
    put(&mut out, &0u16.to_be_bytes());
    write_compound_body(&mut out, map);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> BTreeMap<String, Value> {
        let mut inner = BTreeMap::new();
        inner.insert("b".into(), Value::Byte(-5));
        inner.insert("s".into(), Value::Short(300));
        inner.insert("i".into(), Value::Int(-7));
        inner.insert("l".into(), Value::Long(-1));
        inner.insert("f".into(), Value::Float(1.5));
        inner.insert("d".into(), Value::Double(-0.25));
        inner.insert("ba".into(), Value::ByteArray(vec![1, 2, 3]));
        inner.insert("str".into(), Value::String("hi".into()));
        inner.insert("list".into(), Value::List(vec![Value::Int(1), Value::Int(2)]));
        inner.insert("ia".into(), Value::IntArray(vec![9, -9]));
        inner.insert("la".into(), Value::LongArray(vec![1, 2, 3]));
        let mut root = BTreeMap::new();
        root.insert("".into(), Value::Compound(inner));
        root
    }

    #[test]
    fn nbt_round_trip_all_tags() {
        let s = sample();
        let bytes = write_root(&s);
        let back = Reader::new(&bytes).root().unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn list_of_compounds_round_trips() {
        let mut e = BTreeMap::new();
        e.insert("Name".into(), Value::String("stone".into()));
        let mut root = BTreeMap::new();
        root.insert("palette".into(), Value::List(vec![Value::Compound(e.clone())]));
        let bytes = write_root(&root);
        let back = Reader::new(&bytes).root().unwrap();
        assert_eq!(back, root);
    }
}
