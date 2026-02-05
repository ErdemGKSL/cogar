//! Binary reading and writing utilities for the Ogar protocol.
//!
//! All values are little-endian.

use bytes::{Buf, BufMut, Bytes, BytesMut};

/// A reader for parsing binary protocol messages.
#[derive(Debug)]
pub struct BinaryReader {
    buf: Bytes,
}

impl BinaryReader {
    /// Create a new reader from raw bytes.
    pub fn new(data: impl Into<Bytes>) -> Self {
        Self { buf: data.into() }
    }

    /// Returns remaining bytes.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.buf.remaining()
    }

    /// Skip `n` bytes.
    #[inline]
    pub fn skip(&mut self, n: usize) {
        self.buf.advance(n.min(self.buf.remaining()));
    }

    #[inline]
    pub fn get_u8(&mut self) -> u8 {
        self.buf.get_u8()
    }
    
    /// Safe version that returns None if not enough data
    #[inline]
    pub fn try_get_u8(&mut self) -> Option<u8> {
        if self.buf.remaining() >= 1 {
            Some(self.buf.get_u8())
        } else {
            None
        }
    }

    #[inline]
    pub fn get_i8(&mut self) -> i8 {
        self.buf.get_i8()
    }

    #[inline]
    pub fn get_u16(&mut self) -> u16 {
        self.buf.get_u16_le()
    }
    
    /// Safe version that returns None if not enough data
    #[inline]
    pub fn try_get_u16(&mut self) -> Option<u16> {
        if self.buf.remaining() >= 2 {
            Some(self.buf.get_u16_le())
        } else {
            None
        }
    }

    #[inline]
    pub fn get_i16(&mut self) -> i16 {
        self.buf.get_i16_le()
    }

    #[inline]
    pub fn get_u32(&mut self) -> u32 {
        self.buf.get_u32_le()
    }
    
    /// Safe version that returns None if not enough data
    #[inline]
    pub fn try_get_u32(&mut self) -> Option<u32> {
        if self.buf.remaining() >= 4 {
            Some(self.buf.get_u32_le())
        } else {
            None
        }
    }

    #[inline]
    pub fn get_i32(&mut self) -> i32 {
        self.buf.get_i32_le()
    }
    
    /// Safe version that returns None if not enough data
    #[inline]
    pub fn try_get_i32(&mut self) -> Option<i32> {
        if self.buf.remaining() >= 4 {
            Some(self.buf.get_i32_le())
        } else {
            None
        }
    }

    #[inline]
    pub fn get_f32(&mut self) -> f32 {
        self.buf.get_f32_le()
    }
    
    /// Safe version that returns None if not enough data
    #[inline]
    pub fn try_get_f32(&mut self) -> Option<f32> {
        if self.buf.remaining() >= 4 {
            Some(self.buf.get_f32_le())
        } else {
            None
        }
    }

    #[inline]
    pub fn get_f64(&mut self) -> f64 {
        self.buf.get_f64_le()
    }
    
    /// Safe version that returns None if not enough data
    #[inline]
    pub fn try_get_f64(&mut self) -> Option<f64> {
        if self.buf.remaining() >= 8 {
            Some(self.buf.get_f64_le())
        } else {
            None
        }
    }

    /// Read a null-terminated UTF-8 string.
    pub fn get_string_utf8(&mut self) -> String {
        let mut bytes = Vec::new();
        while self.buf.has_remaining() {
            let b = self.buf.get_u8();
            if b == 0 {
                break;
            }
            bytes.push(b);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Read a null-terminated UTF-16 (UCS-2) string.
    pub fn get_string_unicode(&mut self) -> String {
        let mut chars = Vec::new();
        while self.buf.remaining() >= 2 {
            let c = self.buf.get_u16_le();
            if c == 0 {
                break;
            }
            chars.push(c);
        }
        String::from_utf16_lossy(&chars)
    }
}

/// A writer for building binary protocol messages.
#[derive(Debug, Default)]
pub struct BinaryWriter {
    buf: BytesMut,
}

impl BinaryWriter {
    /// Create a new writer with default capacity.
    pub fn new() -> Self {
        Self::with_capacity(256)
    }

    /// Create a new writer with the specified capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: BytesMut::with_capacity(capacity),
        }
    }

    /// Returns the current length.
    #[inline]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Returns true if empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    #[inline]
    pub fn put_u8(&mut self, v: u8) {
        self.buf.put_u8(v);
    }

    #[inline]
    pub fn put_i8(&mut self, v: i8) {
        self.buf.put_i8(v);
    }

    #[inline]
    pub fn put_u16(&mut self, v: u16) {
        self.buf.put_u16_le(v);
    }

    #[inline]
    pub fn put_i16(&mut self, v: i16) {
        self.buf.put_i16_le(v);
    }

    #[inline]
    pub fn put_u32(&mut self, v: u32) {
        self.buf.put_u32_le(v);
    }

    #[inline]
    pub fn put_i32(&mut self, v: i32) {
        self.buf.put_i32_le(v);
    }

    #[inline]
    pub fn put_f32(&mut self, v: f32) {
        self.buf.put_f32_le(v);
    }

    #[inline]
    pub fn put_f64(&mut self, v: f64) {
        self.buf.put_f64_le(v);
    }

    /// Write a null-terminated UTF-8 string.
    pub fn put_string_utf8(&mut self, s: &str) {
        self.buf.put_slice(s.as_bytes());
        self.buf.put_u8(0);
    }

    /// Write a null-terminated UTF-16 (UCS-2) string.
    pub fn put_string_unicode(&mut self, s: &str) {
        for c in s.encode_utf16() {
            self.buf.put_u16_le(c);
        }
        self.buf.put_u16_le(0);
    }

    /// Write raw bytes.
    pub fn put_slice(&mut self, data: &[u8]) {
        self.buf.put_slice(data);
    }

    /// Consume the writer and return the built buffer.
    pub fn finish(self) -> Bytes {
        self.buf.freeze()
    }

    /// Get current buffer as a slice.
    pub fn as_slice(&self) -> &[u8] {
        &self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_roundtrip_u32() {
        let mut w = BinaryWriter::new();
        w.put_u32(0xDEADBEEF);
        let data = w.finish();
        let mut r = BinaryReader::new(data);
        assert_eq!(r.get_u32(), 0xDEADBEEF);
    }

    #[test]
    fn test_string_utf8() {
        let mut w = BinaryWriter::new();
        w.put_string_utf8("hello");
        let data = w.finish();
        let mut r = BinaryReader::new(data);
        assert_eq!(r.get_string_utf8(), "hello");
    }
}
