//! Network little-endian NBT read in place.
//!
//! [`read_root`] checks a payload and returns an [`NbtDocument`] whose views
//! decode values from the borrowed bytes on access, so no tree is built. The only
//! allocation is one 8-byte [`Span`] per list, compound or numeric-array compound
//! field, which lets lookups step over a field without walking it. Every such field
//! takes at least three wire bytes, so a document allocates at most 8/3 bytes per wire
//! byte whatever its shape.

/// Nesting a definition may use before it is treated as malformed.
const MAX_NBT_DEPTH: usize = 32;

const TAG_END: u8 = 0;
const TAG_BYTE: u8 = 1;
const TAG_SHORT: u8 = 2;
const TAG_INT: u8 = 3;
const TAG_LONG: u8 = 4;
const TAG_FLOAT: u8 = 5;
const TAG_DOUBLE: u8 = 6;
const TAG_BYTE_ARRAY: u8 = 7;
const TAG_STRING: u8 = 8;
const TAG_LIST: u8 = 9;
const TAG_COMPOUND: u8 = 10;
const TAG_INT_ARRAY: u8 = 11;
const TAG_LONG_ARRAY: u8 = 12;

/// Checks a named root compound; `None` for truncation, invalid UTF-8, an unknown
/// tag or excess nesting. Bytes after the root are ignored.
pub(crate) fn read_root(bytes: &[u8]) -> Option<NbtDocument<'_>> {
    let mut cursor = Cursor { bytes, position: 0 };
    if cursor.u8()? != TAG_COMPOUND {
        return None;
    }
    cursor.str()?;
    let root = cursor.position;
    let mut counting = Validator {
        cursor: cursor.clone(),
        spans: 0,
        record: None,
    };
    counting.payload(TAG_COMPOUND, 0)?;
    // The counting walk sizes the spans exactly, so recording them never regrows the
    // vector. Offsets past `u32` keep no spans, so their lookups walk the fields they skip.
    let indexed = u32::try_from(bytes.len()).is_ok();
    let mut spans = Vec::new();
    if indexed && counting.spans > 0 {
        spans.reserve_exact(counting.spans);
        Validator {
            cursor,
            spans: 0,
            record: Some(&mut spans),
        }
        .payload(TAG_COMPOUND, 0)?;
    }
    Some(NbtDocument {
        bytes,
        root,
        spans,
        indexed,
    })
}

/// A checked network NBT payload that [`Nbt`] views read from.
pub(crate) struct NbtDocument<'a> {
    bytes: &'a [u8],
    /// Payload offset of the root compound.
    root: usize,
    /// One per compound field whose value is a list, compound or numeric array, in
    /// wire order. Views carry the index of the first span inside them, so stepping
    /// over a field reads its span directly.
    spans: Vec<Span>,
    /// Whether `spans` covers every such field.
    indexed: bool,
}

/// Where a compound field's payload ends and the index of the first span after it.
#[derive(Clone, Copy, Debug)]
struct Span {
    end: u32,
    next: u32,
}

/// A decoded value. Byte, int and long arrays keep no contents.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Nbt<'a> {
    Byte(i8),
    Short(i64),
    Int(i64),
    Long(i64),
    /// A float or double tag.
    Float(f64),
    String(&'a str),
    Other,
    List(NbtList<'a>),
    Compound(NbtCompound<'a>),
}

/// A list view; elements decode as they are iterated.
#[derive(Clone, Copy)]
pub(crate) struct NbtList<'a> {
    document: &'a NbtDocument<'a>,
    element: u8,
    len: usize,
    /// Offset of the first element.
    start: usize,
    /// Index of the first span inside the list.
    span: u32,
}

/// A compound view; fields decode as they are looked up or iterated.
#[derive(Clone, Copy)]
pub(crate) struct NbtCompound<'a> {
    document: &'a NbtDocument<'a>,
    /// Offset of the first field's tag.
    start: usize,
    /// Index of the first span inside the compound.
    span: u32,
}

impl NbtDocument<'_> {
    /// The root compound.
    pub(crate) fn root(&self) -> Nbt<'_> {
        Nbt::Compound(NbtCompound {
            document: self,
            start: self.root,
            span: 0,
        })
    }
}

impl<'a> NbtDocument<'a> {
    fn cursor(&self, position: usize) -> Cursor<'a> {
        Cursor {
            bytes: self.bytes,
            position,
        }
    }

    /// Decodes the `tag` value whose payload starts at `position`; `span` is the index
    /// of the first span inside it.
    fn value(&'a self, tag: u8, position: usize, span: u32) -> Option<Nbt<'a>> {
        let mut cursor = self.cursor(position);
        Some(match tag {
            TAG_BYTE => Nbt::Byte(cursor.u8()? as i8),
            TAG_SHORT => Nbt::Short(i64::from(i16::from_le_bytes(cursor.array()?))),
            TAG_INT => Nbt::Int(zigzag(cursor.var_u64(5)?)),
            TAG_LONG => Nbt::Long(zigzag(cursor.var_u64(10)?)),
            TAG_FLOAT => Nbt::Float(f64::from(f32::from_le_bytes(cursor.array()?))),
            TAG_DOUBLE => Nbt::Float(f64::from_le_bytes(cursor.array()?)),
            TAG_STRING => Nbt::String(cursor.str()?),
            TAG_BYTE_ARRAY | TAG_INT_ARRAY | TAG_LONG_ARRAY => Nbt::Other,
            TAG_LIST => {
                let element = cursor.u8()?;
                let len = cursor.length()?;
                Nbt::List(NbtList {
                    document: self,
                    element,
                    len,
                    start: cursor.position,
                    span,
                })
            }
            TAG_COMPOUND => Nbt::Compound(NbtCompound {
                document: self,
                start: position,
                span,
            }),
            _ => return None,
        })
    }

    /// Index of the first span inside a compound field's value, given the field's own
    /// span index.
    fn inner_span(&self, tag: u8, span: u32) -> u32 {
        if self.indexed && has_span(tag) {
            span + 1
        } else {
            span
        }
    }

    /// End of a compound field's `tag` payload at `position` and the index of the first
    /// span after it; `span` is the field's own span when it has one.
    fn field_end(&self, tag: u8, position: usize, span: u32) -> Option<(usize, u32)> {
        if self.indexed && has_span(tag) {
            let span = self.spans.get(span as usize)?;
            return Some((span.end as usize, span.next));
        }
        self.value_end(tag, position, span)
    }

    /// End of the `tag` payload at `position` and the index of the first span after it;
    /// `span` is the index of the first span inside it. Nested compound fields are
    /// stepped over by their spans.
    fn value_end(&self, tag: u8, position: usize, mut span: u32) -> Option<(usize, u32)> {
        let mut cursor = self.cursor(position);
        match tag {
            TAG_INT => {
                cursor.var_u64(5)?;
            }
            TAG_LONG => {
                cursor.var_u64(10)?;
            }
            TAG_BYTE_ARRAY => {
                let length = cursor.length()?;
                cursor.skip(length)?;
            }
            TAG_STRING => {
                let length = cursor.string_length()?;
                cursor.skip(length)?;
            }
            TAG_LIST => {
                let element = cursor.u8()?;
                let len = cursor.length()?;
                if let Some(size) = fixed_size(element) {
                    cursor.skip(len.checked_mul(size)?)?;
                } else {
                    for _ in 0..len {
                        (cursor.position, span) = self.value_end(element, cursor.position, span)?;
                    }
                }
            }
            TAG_COMPOUND => loop {
                let child = cursor.u8()?;
                if child == TAG_END {
                    break;
                }
                let length = cursor.string_length()?;
                cursor.skip(length)?;
                (cursor.position, span) = self.field_end(child, cursor.position, span)?;
            },
            TAG_INT_ARRAY | TAG_LONG_ARRAY => {
                let max_bytes = if tag == TAG_INT_ARRAY { 5 } else { 10 };
                for _ in 0..cursor.length()? {
                    cursor.var_u64(max_bytes)?;
                }
            }
            _ => cursor.skip(fixed_size(tag)?)?,
        }
        Some((cursor.position, span))
    }
}

/// Whether a compound field of this tag records a span: those whose end is not
/// found from a length prefix alone.
fn has_span(tag: u8) -> bool {
    matches!(
        tag,
        TAG_LIST | TAG_COMPOUND | TAG_INT_ARRAY | TAG_LONG_ARRAY
    )
}

/// Wire size of a fixed-width payload.
fn fixed_size(tag: u8) -> Option<usize> {
    match tag {
        TAG_BYTE => Some(1),
        TAG_SHORT => Some(2),
        TAG_FLOAT => Some(4),
        TAG_DOUBLE => Some(8),
        _ => None,
    }
}

impl<'a> Nbt<'a> {
    /// The first field named `name`, when this is a compound.
    pub(crate) fn field(self, name: &str) -> Option<Nbt<'a>> {
        match self {
            Self::Compound(compound) => compound.field(name),
            _ => None,
        }
    }

    /// Whether more than one compound field declares `name`.
    pub(crate) fn duplicate_field(self, name: &str) -> bool {
        match self {
            Self::Compound(compound) => compound.duplicate_field(name),
            _ => false,
        }
    }

    /// The list field `name`; empty when it is missing or not a list.
    pub(crate) fn list(self, name: &str) -> NbtList<'a> {
        match self.field(name) {
            Some(Self::List(items)) => items,
            _ => NbtList::empty(),
        }
    }

    /// The text of a string tag.
    pub(crate) fn as_str(self) -> Option<&'a str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Any numeric tag's value as `f64`.
    pub(crate) fn number(self) -> Option<f64> {
        match self {
            Self::Byte(value) => Some(f64::from(value)),
            Self::Short(value) | Self::Int(value) | Self::Long(value) => Some(value as f64),
            Self::Float(value) => Some(value),
            _ => None,
        }
    }

    /// Reads only an Int tag within its signed 32-bit range, without numeric coercion.
    pub(crate) fn int32(self) -> Option<i32> {
        match self {
            Self::Int(value) => i32::try_from(value).ok(),
            _ => None,
        }
    }
}

impl<'a> NbtList<'a> {
    /// A list with no elements.
    fn empty() -> Self {
        static EMPTY: NbtDocument<'static> = NbtDocument {
            bytes: &[],
            root: 0,
            spans: Vec::new(),
            indexed: false,
        };
        NbtList {
            document: &EMPTY,
            element: TAG_END,
            len: 0,
            start: 0,
            span: 0,
        }
    }

    /// The declared element count.
    pub(crate) fn len(self) -> usize {
        self.len
    }

    pub(crate) fn is_empty(self) -> bool {
        self.len == 0
    }

    /// Decodes the elements in order.
    pub(crate) fn iter(self) -> NbtListIter<'a> {
        NbtListIter {
            document: self.document,
            element: self.element,
            remaining: self.len,
            position: self.start,
            span: self.span,
        }
    }
}

impl<'a> IntoIterator for NbtList<'a> {
    type Item = Nbt<'a>;
    type IntoIter = NbtListIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl std::fmt::Debug for NbtList<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

/// Decodes a list's elements in order.
#[derive(Clone)]
pub(crate) struct NbtListIter<'a> {
    document: &'a NbtDocument<'a>,
    element: u8,
    remaining: usize,
    position: usize,
    /// Index of the first span inside the next element.
    span: u32,
}

impl<'a> Iterator for NbtListIter<'a> {
    type Item = Nbt<'a>;

    fn next(&mut self) -> Option<Nbt<'a>> {
        if self.remaining == 0 {
            return None;
        }
        let value = self.document.value(self.element, self.position, self.span);
        let end = self
            .document
            .value_end(self.element, self.position, self.span);
        match value.zip(end) {
            Some((value, (end, span))) => {
                (self.position, self.span) = (end, span);
                self.remaining -= 1;
                Some(value)
            }
            None => {
                self.remaining = 0;
                None
            }
        }
    }

    fn nth(&mut self, count: usize) -> Option<Nbt<'a>> {
        if count >= self.remaining {
            self.remaining = 0;
            return None;
        }
        match fixed_size(self.element) {
            Some(size) => {
                self.position += count * size;
                self.remaining -= count;
            }
            None => {
                for _ in 0..count {
                    self.next()?;
                }
            }
        }
        self.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.remaining))
    }
}

impl<'a> NbtCompound<'a> {
    /// The first field named `name`.
    pub(crate) fn field(self, name: &str) -> Option<Nbt<'a>> {
        self.raw_fields()
            .find(|field| field.key == name.as_bytes())
            .and_then(|field| self.document.value(field.tag, field.position, field.span))
    }

    /// Whether more than one field declares `name`.
    pub(crate) fn duplicate_field(self, name: &str) -> bool {
        self.raw_fields()
            .filter(|field| field.key == name.as_bytes())
            .nth(1)
            .is_some()
    }

    pub(crate) fn is_empty(self) -> bool {
        self.raw_fields().next().is_none()
    }

    /// Fields in wire order, duplicates included.
    pub(crate) fn iter(self) -> impl Iterator<Item = (&'a str, Nbt<'a>)> + Clone {
        let document = self.document;
        self.raw_fields().map_while(move |field| {
            Some((
                std::str::from_utf8(field.key).ok()?,
                document.value(field.tag, field.position, field.span)?,
            ))
        })
    }

    /// Each field's name, tag and payload location, without decoding names or values.
    fn raw_fields(self) -> RawFields<'a> {
        RawFields {
            document: self.document,
            position: Some(self.start),
            span: self.span,
        }
    }
}

impl std::fmt::Debug for NbtCompound<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_map().entries(self.iter()).finish()
    }
}

/// Steps over a compound's fields by their spans.
#[derive(Clone)]
struct RawFields<'a> {
    document: &'a NbtDocument<'a>,
    /// Offset of the next field's tag; `None` once the end tag or a decode failure is reached.
    position: Option<usize>,
    /// Index of the next field's span, or of the first span after it.
    span: u32,
}

/// One undecoded compound field.
struct RawField<'a> {
    key: &'a [u8],
    tag: u8,
    /// Payload offset.
    position: usize,
    /// Index of the first span inside the value.
    span: u32,
}

impl<'a> Iterator for RawFields<'a> {
    type Item = RawField<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut cursor = self.document.cursor(self.position.take()?);
        let tag = cursor.u8()?;
        if tag == TAG_END {
            return None;
        }
        let length = cursor.string_length()?;
        let key = cursor.take(length)?;
        let position = cursor.position;
        let field = RawField {
            key,
            tag,
            position,
            span: self.document.inner_span(tag, self.span),
        };
        let (end, span) = self.document.field_end(tag, position, self.span)?;
        (self.position, self.span) = (Some(end), span);
        Some(field)
    }
}

fn zigzag(raw: u64) -> i64 {
    ((raw >> 1) as i64) ^ -((raw & 1) as i64)
}

/// Network little-endian NBT primitives: VarInt lengths and zigzag VarInt ints/longs.
#[derive(Clone)]
struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let bytes = self
            .bytes
            .get(self.position..self.position.checked_add(count)?)?;
        self.position += count;
        Some(bytes)
    }

    fn skip(&mut self, count: usize) -> Option<()> {
        self.take(count).map(drop)
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }

    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    fn var_u64(&mut self, max_bytes: usize) -> Option<u64> {
        let mut value = 0_u64;
        for index in 0..max_bytes {
            let byte = self.u8()?;
            let bits = if max_bytes == 5 { u32::BITS } else { u64::BITS };
            let shift = index * 7;
            let payload = u64::from(byte & 0x7f);
            if shift >= bits as usize || payload > (u64::MAX >> (u64::BITS - bits)) >> shift {
                return None;
            }
            value |= payload << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    /// A zigzag list or array length, which cannot exceed the bytes left.
    fn length(&mut self) -> Option<usize> {
        let raw = self.var_u64(5)? as u32;
        let value = ((raw >> 1) as i32) ^ -((raw & 1) as i32);
        let length = usize::try_from(value).ok()?;
        (length <= self.bytes.len() - self.position).then_some(length)
    }

    fn string_length(&mut self) -> Option<usize> {
        usize::try_from(self.var_u64(5)?).ok()
    }

    fn str(&mut self) -> Option<&'a str> {
        let length = self.string_length()?;
        std::str::from_utf8(self.take(length)?).ok()
    }
}

/// Walks a payload once to check it, counting or recording compound field spans.
struct Validator<'a, 'r> {
    cursor: Cursor<'a>,
    /// Compound fields that take a span.
    spans: usize,
    /// Receives each span in wire order when set.
    record: Option<&'r mut Vec<Span>>,
}

impl Validator<'_, '_> {
    fn payload(&mut self, tag: u8, depth: usize) -> Option<()> {
        if depth > MAX_NBT_DEPTH {
            return None;
        }
        let cursor = &mut self.cursor;
        match tag {
            TAG_INT => {
                cursor.var_u64(5)?;
            }
            TAG_LONG => {
                cursor.var_u64(10)?;
            }
            TAG_BYTE_ARRAY => {
                let length = cursor.length()?;
                cursor.skip(length)?;
            }
            TAG_STRING => {
                cursor.str()?;
            }
            TAG_LIST => {
                let element = cursor.u8()?;
                let len = cursor.length()?;
                match fixed_size(element) {
                    Some(_) if len > 0 && depth + 1 > MAX_NBT_DEPTH => return None,
                    Some(size) => cursor.skip(len.checked_mul(size)?)?,
                    None => {
                        for _ in 0..len {
                            self.payload(element, depth + 1)?;
                        }
                    }
                }
            }
            TAG_COMPOUND => loop {
                let child = self.cursor.u8()?;
                if child == TAG_END {
                    break;
                }
                self.cursor.str()?;
                if !has_span(child) {
                    self.payload(child, depth + 1)?;
                    continue;
                }
                self.spans += 1;
                let slot = self.record.as_deref_mut().map(|spans| {
                    spans.push(Span { end: 0, next: 0 });
                    spans.len() - 1
                });
                self.payload(child, depth + 1)?;
                if let (Some(slot), Some(spans)) = (slot, self.record.as_deref_mut()) {
                    // `read_root` records only when every offset fits `u32`, and spans
                    // number fewer than bytes.
                    spans[slot] = Span {
                        end: self.cursor.position as u32,
                        next: spans.len() as u32,
                    };
                }
            },
            TAG_INT_ARRAY | TAG_LONG_ARRAY => {
                let max_bytes = if tag == TAG_INT_ARRAY { 5 } else { 10 };
                for _ in 0..cursor.length()? {
                    cursor.var_u64(max_bytes)?;
                }
            }
            _ => cursor.skip(fixed_size(tag)?)?,
        }
        Some(())
    }
}

#[cfg(test)]
pub(crate) mod test_tree;

#[cfg(test)]
mod tests;
