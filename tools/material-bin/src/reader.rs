//! Little-endian reads over a byte slice that fail instead of panicking past its end.

use anyhow::{Context, Result};

/// A cursor over a byte slice.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    /// A reader at the start of `bytes`.
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// Whether every byte has been read.
    pub(crate) fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The next `count` bytes.
    pub(crate) fn bytes(&mut self, count: usize) -> Result<&'a [u8]> {
        let (head, tail) = self
            .bytes
            .split_at_checked(count)
            .context("unexpected end of data")?;
        self.bytes = tail;
        Ok(head)
    }

    /// The next `N` bytes as an array.
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.bytes(N)?.try_into().expect("N bytes were read"))
    }

    /// The next little-endian `u8`.
    pub(crate) fn u8(&mut self) -> Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    /// The next little-endian `u16`.
    pub(crate) fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    /// The next little-endian `u32`.
    pub(crate) fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    /// The next little-endian `u64`.
    pub(crate) fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    /// The next little-endian `f32`.
    pub(crate) fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    /// A string prefixed by its `u32` byte length.
    pub(crate) fn string(&mut self) -> Result<String> {
        let length = self.u32()? as usize;
        let bytes = self.bytes(length)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    /// A value preceded by a presence byte.
    pub(crate) fn optional<T>(
        &mut self,
        read: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<Option<T>> {
        if self.u8()? == 0 {
            Ok(None)
        } else {
            read(self).map(Some)
        }
    }
}
