//! Test builders for network little-endian NBT.

/// An owned value that tests build and read back through the wire reader.
///
/// `Float` encodes as a double so its value survives exactly; `Other` encodes as an
/// empty byte array. Lists must hold one kind, as on the wire.
#[derive(Debug, Clone)]
pub(crate) enum Nbt {
    Byte(i8),
    Short(i64),
    Int(i64),
    Long(i64),
    Float(f64),
    String(String),
    Other,
    List(Vec<Nbt>),
    Compound(Vec<(String, Nbt)>),
}

impl Nbt {
    fn tag(&self) -> u8 {
        match self {
            Self::Byte(_) => 1,
            Self::Short(_) => 2,
            Self::Int(_) => 3,
            Self::Long(_) => 4,
            Self::Float(_) => 6,
            Self::Other => 7,
            Self::String(_) => 8,
            Self::List(_) => 9,
            Self::Compound(_) => 10,
        }
    }

    fn encode_payload(&self, bytes: &mut Vec<u8>) {
        match self {
            Self::Byte(value) => bytes.push(*value as u8),
            Self::Short(value) => {
                bytes.extend_from_slice(&i16::try_from(*value).expect("short fits").to_le_bytes())
            }
            Self::Int(value) => {
                let value = i32::try_from(*value).expect("int fits");
                var_u64(bytes, u64::from(((value << 1) ^ (value >> 31)) as u32));
            }
            Self::Long(value) => var_u64(bytes, ((value << 1) ^ (value >> 63)) as u64),
            Self::Float(value) => bytes.extend_from_slice(&value.to_le_bytes()),
            Self::String(value) => string(bytes, value),
            Self::Other => length(bytes, 0),
            Self::List(items) => {
                let element = items.first().map_or(0, Self::tag);
                assert!(
                    items.iter().all(|item| item.tag() == element),
                    "NBT list elements share one tag"
                );
                bytes.push(element);
                length(bytes, items.len());
                for item in items {
                    item.encode_payload(bytes);
                }
            }
            Self::Compound(fields) => {
                for (name, value) in fields {
                    bytes.push(value.tag());
                    string(bytes, name);
                    value.encode_payload(bytes);
                }
                bytes.push(0);
            }
        }
    }

    /// Encodes a root compound named `""` holding this value as its only field, `""`.
    pub(crate) fn encode_wrapped(&self) -> Vec<u8> {
        let mut bytes = vec![10, 0];
        Self::Compound(vec![(String::new(), self.clone())]).encode_payload(&mut bytes);
        bytes
    }

    /// Reads this value back through [`super::read_root`]; the encoding is leaked so the
    /// view can outlive the test's temporaries.
    pub(crate) fn view(&self) -> super::Nbt<'static> {
        let bytes: &'static [u8] = Vec::leak(self.encode_wrapped());
        let document = Box::leak(Box::new(
            super::read_root(bytes).expect("encoded NBT reads"),
        ));
        document.root().field("").expect("wrapped value")
    }
}

/// Appends `value` as an unsigned VarInt.
fn var_u64(bytes: &mut Vec<u8>, mut value: u64) {
    loop {
        let next = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            bytes.push(next);
            return;
        }
        bytes.push(next | 0x80);
    }
}

/// Appends a VarInt-prefixed name or string.
fn string(bytes: &mut Vec<u8>, value: &str) {
    var_u64(bytes, value.len() as u64);
    bytes.extend_from_slice(value.as_bytes());
}

/// Appends a zigzag VarInt list or array length.
fn length(bytes: &mut Vec<u8>, count: usize) {
    var_u64(
        bytes,
        u64::from(u32::try_from(count).expect("test length fits") << 1),
    );
}

/// Named fields of about `wire_bytes` each, one per shape that a per-element tree
/// would expand most, labelled for assertion messages.
///
/// Every element or field is one to three wire bytes, so a decoder that allocates
/// per element costs tens of bytes per wire byte on each of them.
pub(crate) fn amplifying_fields(name: &str, wire_bytes: usize) -> Vec<(&'static str, Vec<u8>)> {
    let list = |element: u8, element_bytes: &[u8]| {
        let count = wire_bytes / element_bytes.len();
        let mut bytes = vec![9];
        string(&mut bytes, name);
        bytes.push(element);
        length(&mut bytes, count);
        for _ in 0..count {
            bytes.extend_from_slice(element_bytes);
        }
        bytes
    };
    let compound = |field: &[u8]| {
        let mut bytes = vec![10];
        string(&mut bytes, name);
        for _ in 0..wire_bytes / field.len() {
            bytes.extend_from_slice(field);
        }
        bytes.push(0);
        bytes
    };
    vec![
        ("byte list", list(1, &[0])),
        ("int list", list(3, &[0])),
        ("long list", list(4, &[0])),
        ("string list", list(8, &[0])),
        ("byte array list", list(7, &[0])),
        ("compound list", list(10, &[0])),
        ("list list", list(9, &[0, 0])),
        ("byte fields", compound(&[1, 0, 0])),
        ("string fields", compound(&[8, 0, 0])),
        ("compound fields", compound(&[10, 0, 0])),
        ("list fields", compound(&[9, 0, 0, 0])),
        ("int array fields", compound(&[11, 0, 0])),
    ]
}
