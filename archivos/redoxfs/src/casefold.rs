//! Caseless names, for the directories of DashFS 2 marked with `NodeFlags::CASEFOLD`.
//!
//! The name is stored as it was written; lookups use its key. Two names are the same if their keys are equal.
//! The key is NFD(casefold(NFD(c))) for each character, followed by canonical ordering of the combining marks, so
//! "Árbol", "ÁRBOL" and "árbol" (with the accent as one character or as two) are the same name, and so are
//! "Straße" and "STRASSE". The tables are frozen (`casefold_data.rs`, Unicode 16): changing them changes keys of
//! names already on disk, which needs a new DashFS version.

use alloc::string::String;
use alloc::vec::Vec;

use crate::casefold_data::{COMBINING, FOLD_INDEX, FOLD_TEXT};
use crate::htree::HTreeHash;
use crate::node::{Node, NodeFlags};

// Hangul syllables decompose by formula, not by table (Unicode chapter 3.12).
const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = 19 * N_COUNT;

/// How a directory compares the names of its entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Names {
    /// Byte by byte, as in RedoxFS and Unix.
    Exact,
    /// By key, as in AmigaOS, Windows and macOS.
    Caseless,
}

impl Names {
    pub fn of(dir: &Node) -> Self {
        if dir.flags().contains(NodeFlags::CASEFOLD) {
            Names::Caseless
        } else {
            Names::Exact
        }
    }

    /// The H-tree hash of a name in a directory of this kind.
    pub fn hash(self, name: &str) -> HTreeHash {
        match self {
            Names::Exact => HTreeHash::from_name(name),
            Names::Caseless => HTreeHash::from_name(&key(name)),
        }
    }

    /// What `name` is compared against: the name itself, or its key.
    pub fn wanted(self, name: &str) -> Wanted<'_> {
        match self {
            Names::Exact => Wanted::Exact(name),
            Names::Caseless => Wanted::Key(key(name)),
        }
    }
}

/// A name being looked for, ready to compare against many entries.
pub enum Wanted<'a> {
    Exact(&'a str),
    Key(String),
}

impl Wanted<'_> {
    pub fn matches(&self, entry: &[u8]) -> bool {
        match self {
            Wanted::Exact(name) => entry == name.as_bytes(),
            Wanted::Key(wanted) => {
                if entry.is_ascii() {
                    // An ASCII key is the name in lower case.
                    entry.len() == wanted.len()
                        && entry
                            .iter()
                            .zip(wanted.bytes())
                            .all(|(a, b)| a.to_ascii_lowercase() == b)
                } else {
                    core::str::from_utf8(entry).is_ok_and(|entry| key(entry) == *wanted)
                }
            }
        }
    }
}

/// The key of a name: equal keys, same name.
pub fn key(name: &str) -> String {
    if name.is_ascii() {
        return name.to_ascii_lowercase();
    }
    let mut out = String::with_capacity(name.len() + 8);
    let mut marks = false;
    for c in name.chars() {
        let cp = c as u32;
        if (S_BASE..S_BASE + S_COUNT).contains(&cp) {
            let s = cp - S_BASE;
            out.push(char::from_u32(L_BASE + s / N_COUNT).unwrap());
            out.push(char::from_u32(V_BASE + (s % N_COUNT) / T_COUNT).unwrap());
            if !s.is_multiple_of(T_COUNT) {
                out.push(char::from_u32(T_BASE + s % T_COUNT).unwrap());
            }
            continue;
        }
        match FOLD_INDEX.binary_search_by_key(&cp, |&(c, _)| c) {
            Ok(i) => {
                let at = FOLD_INDEX[i].1;
                let start = (at >> 8) as usize;
                out.push_str(&FOLD_TEXT[start..start + (at & 0xFF) as usize]);
                marks = true;
            }
            Err(_) => {
                out.push(c);
                marks |= combining_class(c) != 0;
            }
        }
    }
    if marks { canonical_order(out) } else { out }
}

fn combining_class(c: char) -> u8 {
    let cp = c as u32;
    // Code points below U+0300 have class 0.
    if cp < 0x300 {
        return 0;
    }
    match COMBINING.binary_search_by(|&(first, last, _)| {
        if last < cp {
            core::cmp::Ordering::Less
        } else if first > cp {
            core::cmp::Ordering::Greater
        } else {
            core::cmp::Ordering::Equal
        }
    }) {
        Ok(i) => COMBINING[i].2,
        Err(_) => 0,
    }
}

/// Sorts each run of combining marks by class, keeping the order of equal classes.
fn canonical_order(text: String) -> String {
    let mut chars: Vec<(char, u8)> = text.chars().map(|c| (c, combining_class(c))).collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].1 == 0 {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].1 != 0 {
            i += 1;
        }
        chars[start..i].sort_by_key(|&(_, class)| class);
    }
    chars.into_iter().map(|(c, _)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(a: &str, b: &str) -> bool {
        key(a) == key(b)
    }

    #[test]
    fn ascii_and_latin() {
        assert!(same("S:Startup-Sequence", "s:startup-sequence"));
        assert!(same("LIBS:", "Libs:"));
        assert!(same("Árbol", "árbol"));
        assert!(same("ÁRBOL", "a\u{301}rbol"));
        assert!(same("Ñandú", "ñANDÚ"));
        assert!(same("Straße", "STRASSE"));
        assert!(same("ẞ", "ss"));
        assert!(!same("Arbol", "Árbol"));
        assert!(!same("año", "ano"));
        assert!(!same("a", "b"));
    }

    #[test]
    fn other_scripts() {
        assert!(same("ΣΟΦΟΣ", "σοφος"));
        assert!(same("σοφος", "σοφοσ"));
        assert!(same("МОСКВА", "москва"));
        assert!(same("𐐀", "𐐨"));
        // Hangul as one syllable or as letters.
        assert!(same("한", "\u{1112}\u{1161}\u{11AB}"));
        // Default folding, not Turkish: dotless ı is its own letter.
        assert!(!same("ı", "i"));
        assert!(same("K", "\u{212A}"));
    }

    #[test]
    fn marks_in_any_order() {
        // q with dot below and dot above, in both orders.
        assert!(same("q\u{323}\u{307}", "q\u{307}\u{323}"));
        assert!(same("ṩ", "s\u{323}\u{307}"));
        // Same class keeps its order.
        assert!(!same("a\u{301}\u{300}", "a\u{300}\u{301}"));
    }

    #[test]
    fn key_of_key_is_itself() {
        for cp in 0..0x11_0000u32 {
            if let Some(c) = char::from_u32(cp) {
                let mut s = String::new();
                s.push(c);
                let k = key(&s);
                assert_eq!(key(&k), k, "U+{cp:04X}");
            }
        }
    }

    #[test]
    fn tables_are_sorted() {
        assert!(FOLD_INDEX.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(COMBINING.windows(2).all(|w| w[0].1 < w[1].0));
    }

    /// The same keys as Python's unicodedata, on strings mixing scripts, cases and marks.
    #[test]
    fn same_as_python() {
        let text = include_str!("../tests/casefold_vectors.txt");
        let parse = |s: &str| -> String {
            s.split(' ')
                .map(|h| char::from_u32(u32::from_str_radix(h, 16).unwrap()).unwrap())
                .collect()
        };
        let mut n = 0;
        for line in text.lines().filter(|l| !l.starts_with('#')) {
            let (name, wanted) = line.split_once('\t').unwrap();
            let name = parse(name);
            assert_eq!(key(&name), parse(wanted), "{name:?}");
            n += 1;
        }
        assert_eq!(n, 3000);
    }
}
