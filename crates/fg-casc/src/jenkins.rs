//! Jenkins lookup3 `hashlittle2` (Bob Jenkins, public domain): the hash of a
//! file's path the game's root manifest keeps. Ported from the reference
//! implementation (burtleburtle.net/bob/c/lookup3.c); the vectors below are
//! its own.

/// `mix`, the reversible mixing of three 32-bit words.
fn mix(a: &mut u32, b: &mut u32, c: &mut u32) {
    let rot = |x: u32, k: u32| x.rotate_left(k);
    *a = a.wrapping_sub(*c);
    *a ^= rot(*c, 4);
    *c = c.wrapping_add(*b);
    *b = b.wrapping_sub(*a);
    *b ^= rot(*a, 6);
    *a = a.wrapping_add(*c);
    *c = c.wrapping_sub(*b);
    *c ^= rot(*b, 8);
    *b = b.wrapping_add(*a);
    *a = a.wrapping_sub(*c);
    *a ^= rot(*c, 16);
    *c = c.wrapping_add(*b);
    *b = b.wrapping_sub(*a);
    *b ^= rot(*a, 19);
    *a = a.wrapping_add(*c);
    *c = c.wrapping_sub(*b);
    *c ^= rot(*b, 4);
    *b = b.wrapping_add(*a);
}

/// `final`, the last mixing of three 32-bit words into `c`.
fn finish(a: &mut u32, b: &mut u32, c: &mut u32) {
    let rot = |x: u32, k: u32| x.rotate_left(k);
    *c ^= *b;
    *c = c.wrapping_sub(rot(*b, 14));
    *a ^= *c;
    *a = a.wrapping_sub(rot(*c, 11));
    *b ^= *a;
    *b = b.wrapping_sub(rot(*a, 25));
    *c ^= *b;
    *c = c.wrapping_sub(rot(*b, 16));
    *a ^= *c;
    *a = a.wrapping_sub(rot(*c, 4));
    *b ^= *a;
    *b = b.wrapping_sub(rot(*a, 14));
    *c ^= *b;
    *c = c.wrapping_sub(rot(*b, 24));
}

/// Hash `key` into `(c, b)`, both seeded. The 8 bytes the game's root manifest
/// keeps are `(c, b)` in this order (the hash "jenkins96").
pub fn hashlittle2(key: &[u8], pc: u32, pb: u32) -> (u32, u32) {
    // The internal state: the three words start from the seed and the length,
    // the third one from the second seed too.
    let mut a = 0xdead_beefu32.wrapping_add(key.len() as u32).wrapping_add(pc);
    let mut b = a;
    let mut c = a.wrapping_add(pb);
    let mut k = key;
    // All but the last 12 bytes, three words at a time.
    while k.len() > 12 {
        a = a.wrapping_add(u32::from_le_bytes(k[..4].try_into().expect("four bytes")));
        b = b.wrapping_add(u32::from_le_bytes(k[4..8].try_into().expect("four bytes")));
        c = c.wrapping_add(u32::from_le_bytes(k[8..12].try_into().expect("four bytes")));
        mix(&mut a, &mut b, &mut c);
        k = &k[12..];
    }
    // The last (possibly partial) block, read as the little-endian words it
    // would be, the missing bytes left out (the reference's fall-through).
    let tail = |from: usize, len: usize| -> u32 { (0..len).fold(0u32, |v, i| v + (u32::from(k[from + i]) << (i * 8))) };
    match k.len() {
        12 => {
            c = c.wrapping_add(tail(8, 4));
            b = b.wrapping_add(tail(4, 4));
            a = a.wrapping_add(tail(0, 4));
        }
        11 => {
            c = c.wrapping_add(tail(8, 3));
            b = b.wrapping_add(tail(4, 4));
            a = a.wrapping_add(tail(0, 4));
        }
        10 => {
            c = c.wrapping_add(tail(8, 2));
            b = b.wrapping_add(tail(4, 4));
            a = a.wrapping_add(tail(0, 4));
        }
        9 => {
            c = c.wrapping_add(tail(8, 1));
            b = b.wrapping_add(tail(4, 4));
            a = a.wrapping_add(tail(0, 4));
        }
        8 => {
            b = b.wrapping_add(tail(4, 4));
            a = a.wrapping_add(tail(0, 4));
        }
        7 => {
            b = b.wrapping_add(tail(4, 3));
            a = a.wrapping_add(tail(0, 4));
        }
        6 => {
            b = b.wrapping_add(tail(4, 2));
            a = a.wrapping_add(tail(0, 4));
        }
        5 => {
            b = b.wrapping_add(tail(4, 1));
            a = a.wrapping_add(tail(0, 4));
        }
        4 => a = a.wrapping_add(tail(0, 4)),
        3 => a = a.wrapping_add(tail(0, 3)),
        2 => a = a.wrapping_add(tail(0, 2)),
        1 => a = a.wrapping_add(tail(0, 1)),
        _ => return (c, b),
    }
    finish(&mut a, &mut b, &mut c);
    (c, b)
}

/// The 8-byte hash of a file's path the root manifest keeps: `hashlittle2` of
/// the upper-cased path with forward slashes turned into backslashes (the
/// game's own normalization), `b` in the low 32 bits and `c` in the high ones.
pub fn hashpath(path: &str) -> [u8; 8] {
    let normalized: String = path
        .chars()
        .map(|c| if c == '/' { '\\' } else { c.to_ascii_uppercase() })
        .collect();
    let (c, b) = hashlittle2(normalized.as_bytes(), 0, 0);
    let mut out = [0u8; 8];
    out[..4].copy_from_slice(&b.to_le_bytes());
    out[4..].copy_from_slice(&c.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reference implementation's own vectors (driver5; `b=1, c=0` there
    /// means the seeds `pc = 0`, `pb = 1`).
    #[test]
    fn reference_vectors() {
        let (c, b) = hashlittle2(b"", 0, 0);
        assert_eq!((c, b), (0xdead_beef, 0xdead_beef));
        let (c, b) = hashlittle2(b"", 0, 0xdead_beef);
        assert_eq!((c, b), (0xbd5b_7dde, 0xdead_beef));
        let (c, b) = hashlittle2(b"", 0xdead_beef, 0xdead_beef);
        assert_eq!((c, b), (0x9c09_3ccd, 0xbd5b_7dde));
        let (c, b) = hashlittle2(b"Four score and seven years ago", 0, 0);
        assert_eq!((c, b), (0x1777_0551, 0xce72_26e6));
        let (c, b) = hashlittle2(b"Four score and seven years ago", 0, 1);
        assert_eq!((c, b), (0xe360_7cae, 0xbd37_1de4));
        let (c, b) = hashlittle2(b"Four score and seven years ago", 1, 0);
        assert_eq!((c, b), (0xcd62_8161, 0x6cbe_a4b3));
    }

    /// The path hash of a known file of the client: the 8 bytes the root
    /// manifest keeps.
    #[test]
    fn path_hash() {
        // (checked against the manifest of an installed client)
        let hash = hashpath("Interface\\Icons\\INV_Sword_04.blp");
        assert_eq!(hash, [0x84, 0xb5, 0xbf, 0xba, 0x81, 0x30, 0x4e, 0x81]);
    }
}
