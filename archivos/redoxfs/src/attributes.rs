//! DashFS 3: attributes of a node, in one block of its own: the Amiga comment ("amiga.comment") and, later, what
//! other systems keep beside a file (extended attributes of the Linux box).
//!
//! The block holds entries one after another: key length (1 byte), value length (2 bytes), key (UTF-8), value.

use alloc::vec::Vec;
use core::{fmt, mem, ops, slice};
use endian_num::Le;

use crate::{BLOCK_SIZE, BlockLevel, BlockTrait};

/// The comment of a file or directory, as in AmigaOS (`SetComment`).
pub const COMMENT: &str = "amiga.comment";

const DATA_SIZE: usize = BLOCK_SIZE as usize - mem::size_of::<Le<u16>>();
const ENTRY_HEADER: usize = 3;

#[repr(C, packed)]
pub struct Attributes {
    /// Bytes of `data` in use.
    len: Le<u16>,
    data: [u8; DATA_SIZE],
}

unsafe impl BlockTrait for Attributes {
    fn empty(level: BlockLevel) -> Option<Self> {
        if level.0 == 0 {
            Some(Self {
                len: 0.into(),
                data: [0; DATA_SIZE],
            })
        } else {
            None
        }
    }
}

impl Attributes {
    /// The entries as (key, value), stopping at anything malformed.
    pub fn entries(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        let used = &self.data[..(self.len.to_ne() as usize).min(DATA_SIZE)];
        let mut at = 0;
        core::iter::from_fn(move || {
            let header = used.get(at..at + ENTRY_HEADER)?;
            let key_len = header[0] as usize;
            let value_len = u16::from_le_bytes([header[1], header[2]]) as usize;
            let key_start = at + ENTRY_HEADER;
            let value_start = key_start + key_len;
            let key = used.get(key_start..value_start)?;
            let value = used.get(value_start..value_start + value_len)?;
            at = value_start + value_len;
            Some((key, value))
        })
    }

    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.entries()
            .find(|(k, _)| *k == key.as_bytes())
            .map(|(_, v)| v)
    }

    pub fn is_empty(&self) -> bool {
        self.len.to_ne() == 0
    }

    /// Sets or removes (`None`) one attribute. False if it does not fit, leaving the block as it was.
    pub fn set(&mut self, key: &str, value: Option<&[u8]>) -> bool {
        if key.is_empty() || key.len() > u8::MAX as usize {
            return false;
        }
        let mut entries: Vec<(Vec<u8>, Vec<u8>)> = self
            .entries()
            .filter(|(k, _)| *k != key.as_bytes())
            .map(|(k, v)| (k.to_vec(), v.to_vec()))
            .collect();
        if let Some(value) = value {
            entries.push((key.as_bytes().to_vec(), value.to_vec()));
        }
        let size: usize = entries
            .iter()
            .map(|(k, v)| ENTRY_HEADER + k.len() + v.len())
            .sum();
        if size > DATA_SIZE {
            return false;
        }
        let mut at = 0;
        for (k, v) in &entries {
            self.data[at] = k.len() as u8;
            self.data[at + 1..at + 3].copy_from_slice(&(v.len() as u16).to_le_bytes());
            at += ENTRY_HEADER;
            self.data[at..at + k.len()].copy_from_slice(k);
            at += k.len();
            self.data[at..at + v.len()].copy_from_slice(v);
            at += v.len();
        }
        self.data[at..].fill(0);
        self.len = (at as u16).into();
        true
    }
}

impl fmt::Debug for Attributes {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_map()
            .entries(
                self.entries()
                    .map(|(k, v)| (alloc::string::String::from_utf8_lossy(k), v.len())),
            )
            .finish()
    }
}

impl ops::Deref for Attributes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe {
            slice::from_raw_parts(
                self as *const Attributes as *const u8,
                mem::size_of::<Attributes>(),
            ) as &[u8]
        }
    }
}

impl ops::DerefMut for Attributes {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe {
            slice::from_raw_parts_mut(
                self as *mut Attributes as *mut u8,
                mem::size_of::<Attributes>(),
            ) as &mut [u8]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size() {
        assert_eq!(mem::size_of::<Attributes>(), BLOCK_SIZE as usize);
    }

    #[test]
    fn set_get_remove() {
        let mut a = Attributes::empty(BlockLevel(0)).unwrap();
        assert!(a.is_empty());
        assert!(a.set(COMMENT, Some("Hola, ñandú".as_bytes())));
        assert!(a.set("user.x", Some(b"")));
        assert_eq!(a.get(COMMENT), Some("Hola, ñandú".as_bytes()));
        assert_eq!(a.get("user.x"), Some(&b""[..]));
        assert!(a.set(COMMENT, Some(b"otro")));
        assert_eq!(a.get(COMMENT), Some(&b"otro"[..]));
        assert_eq!(a.entries().count(), 2);
        assert!(a.set(COMMENT, None));
        assert_eq!(a.get(COMMENT), None);
        assert!(a.set("user.x", None));
        assert!(a.is_empty());
    }

    #[test]
    fn too_big_leaves_it_as_it_was() {
        let mut a = Attributes::empty(BlockLevel(0)).unwrap();
        assert!(a.set("a", Some(&[1; 3000])));
        assert!(!a.set("b", Some(&[2; 2000])));
        assert_eq!(a.get("a"), Some(&[1; 3000][..]));
        assert_eq!(a.get("b"), None);
        assert!(a.set("b", Some(&[2; DATA_SIZE - 3000 - 2 * ENTRY_HEADER - 2])));
        assert!(!a.set("", Some(b"x")));
        assert!(!a.set(&"k".repeat(256), Some(b"x")));
    }
}
