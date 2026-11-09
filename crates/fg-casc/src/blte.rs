//! BLTE, the container of the files in CASC (the game's storage): a header, a
//! table of blocks (each its compressed and decompressed size, and a hash), and
//! the blocks themselves — plain, zlib or LZ4 data, or encrypted (wowdev.wiki/
//! BLTE). The databases of the game's clients are encrypted: their keys are the
//! client's own, shipped with it in its `DBFilesClient\TactKey` tables (what
//! `Storage` reads them into); the cinematics and the like are not read here.

use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::io::Read;

/// The keys the encrypted blocks decrypt with, by the name they ask for (see
/// `Storage::tact_keys`).
pub type Keys = HashMap<[u8; 8], [u8; 16]>;

/// How many bytes a block's entry takes in the table (`rawSize` and
/// `logicalSize`, both 32-bit big-endian, and an MD5).
const BLOCK_INFO: usize = 4 + 4 + 16;

/// Decode a BLTE file (its whole bytes) into the file it contains.
pub fn decode(data: &[u8], keys: &Keys) -> Result<Vec<u8>> {
    if data.len() < 8 || &data[..4] != b"BLTE" {
        bail!("not a BLTE file");
    }
    let be =
        |from: usize, len: usize| -> u64 { data[from..from + len].iter().fold(0u64, |v, b| v << 8 | u64::from(*b)) };
    // The header counts itself: `headerSize` of it, then the blocks' data.
    let header = be(4, 4) as usize;
    if header == 0 {
        // No block table: the whole file is one plain block.
        return Ok(data[8..].to_vec());
    }
    if header < 12 || header > data.len() {
        bail!("a BLTE header out of bounds");
    }
    // The block table: its format byte, how many blocks, then each block's
    // compressed and decompressed size.
    if data[8] != 0x0F {
        bail!("an unknown BLTE table format {:x}", data[8]);
    }
    let blocks = be(9, 3) as usize;
    if 12 + blocks * BLOCK_INFO != header {
        bail!("a BLTE block table of the wrong size");
    }
    let mut out = Vec::new();
    let mut at = header;
    for frame in 0..blocks as u32 {
        let raw = be(12 + frame as usize * BLOCK_INFO, 4) as usize;
        if at + raw > data.len() {
            bail!("a BLTE block out of bounds");
        }
        decode_block(&mut out, &data[at..at + raw], frame, keys)?;
        at += raw;
    }
    Ok(out)
}

/// Decode one block into `out`. Its first byte is its mode: plain data, zlib or
/// LZ4 data, or encrypted — a decrypted block holds another mode-prefixed one.
/// `frame` is the block's index in its file, folded into its encryption.
fn decode_block(out: &mut Vec<u8>, block: &[u8], frame: u32, keys: &Keys) -> Result<()> {
    if block.is_empty() {
        bail!("an empty BLTE block");
    }
    match block[0] {
        // Plain data.
        b'N' => out.extend_from_slice(&block[1..]),
        // Zlib: its two header bytes included.
        b'Z' => {
            flate2::read::ZlibDecoder::new(&block[1..])
                .read_to_end(out)
                .context("a zlib block")?;
        }
        // LZ4: a version byte, a 64-bit decompressed size, a sub-block shift
        // (2^shift bytes a sub-block).
        b'4' => {
            let size = u64::from_be_bytes(block.get(2..10).context("an LZ4 block cut short")?.try_into()?) as usize;
            let shift = u32::from(*block.get(10).context("an LZ4 block cut short")?);
            if shift >= 32 {
                bail!("an LZ4 sub-block shift of {shift}, out of bounds");
            }
            decode_lz4(
                out,
                block.get(11..).context("an LZ4 block cut short")?,
                size,
                1usize << shift,
            )?;
        }
        // Encrypted: its key's name and its IV first, then another block.
        b'E' => {
            let plain = decrypt(block, frame, keys)?;
            decode_block(out, &plain, frame, keys)?;
        }
        // Recursively encoded.
        b'F' => {
            out.extend_from_slice(&decode(block.get(1..).context("an empty BLTE block")?, keys)?);
        }
        other => bail!("an unknown BLTE encoding {:?}", other as char),
    }
    Ok(())
}

/// Decrypt an encrypted block ('E'): its key's name, its IV, then the data under
/// Salsa20. The key comes from `keys`, by name; none matches → the file stays
/// closed (an error).
fn decrypt(block: &[u8], frame: u32, keys: &Keys) -> Result<Vec<u8>> {
    if block.len() < 2 {
        bail!("an encrypted block cut short");
    }
    // The header: the name's length, the name, the IV's length, the IV, the
    // cipher's type ('S': Salsa20), then the data.
    let name_len = usize::from(block[1]);
    let name: [u8; 8] = block
        .get(2..2 + name_len)
        .and_then(|n| n.try_into().ok())
        .context("an encrypted block without a key name")?;
    let mut at = 2 + name_len;
    let iv_len = usize::from(*block.get(at).context("an encrypted block without an IV")?);
    at += 1;
    let iv = &block
        .get(at..at + iv_len)
        .context("an encrypted block with its IV cut short")?[..iv_len.min(8)];
    at += iv_len;
    if *block.get(at).context("an encrypted block without its cipher")? != b'S' {
        bail!("an encrypted block of an unknown cipher, only Salsa20 is read");
    }
    at += 1;
    let Some(key) = keys.get(&name) else {
        bail!("an unknown TACT key {name:02x?}");
    };
    // The IV, padded to the Salsa20 nonce's eight bytes, its first four XORed
    // with the block's index in its file (the frame).
    let mut nonce = [0u8; 8];
    nonce[..iv.len()].copy_from_slice(iv);
    for (i, byte) in nonce.iter_mut().take(4).enumerate() {
        *byte ^= u8::try_from(frame >> (i * 8)).unwrap_or(0);
    }
    Ok(salsa20(key, nonce, &block[at..]))
}

/// Salsa20 (twenty rounds, the game's TACT variant): the key's words twice —
/// the sixteen-byte TACT key used as a thirty-two-byte one — the constants of
/// "expand 16-byte k" at the corners, the nonce and the block counter in
/// between. `data` XOR the keystream.
fn salsa20(key: &[u8; 16], nonce: [u8; 8], data: &[u8]) -> Vec<u8> {
    let word = |b: &[u8]| u32::from_le_bytes(b.try_into().expect("four bytes"));
    let k = [
        word(&key[0..4]),
        word(&key[4..8]),
        word(&key[8..12]),
        word(&key[12..16]),
    ];
    let mut state = [
        0x6170_7865,
        k[0],
        k[1],
        k[2],
        k[3],
        0x3120_646e,
        word(&nonce[0..4]),
        word(&nonce[4..8]),
        0,
        0,
        0x7962_2d36,
        k[0],
        k[1],
        k[2],
        k[3],
        0x6b20_6574,
    ];
    let mut out = data.to_vec();
    let mut stream = block_words(&state);
    for (n, byte) in out.iter_mut().enumerate() {
        if n % 64 == 0 && n > 0 {
            state[8] = (n / 64) as u32; // the block counter, its low word
            stream = block_words(&state);
        }
        *byte ^= stream[n % 64];
    }
    out
}

/// The 64 bytes of one Salsa20 block: twenty rounds on the state, then the
/// state added to the one it started from.
fn block_words(state: &[u32; 16]) -> [u8; 64] {
    let mut x = *state;
    let quarter = |x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize| {
        let rotl = |v: u32, k: u32| v.rotate_left(k);
        x[b] ^= rotl(x[a].wrapping_add(x[d]), 7);
        x[c] ^= rotl(x[a].wrapping_add(x[b]), 9);
        x[d] ^= rotl(x[b].wrapping_add(x[c]), 13);
        x[a] ^= rotl(x[c].wrapping_add(x[d]), 18);
    };
    for _ in 0..10 {
        // The column round, then the row one.
        quarter(&mut x, 0, 4, 8, 12);
        quarter(&mut x, 5, 9, 13, 1);
        quarter(&mut x, 10, 14, 2, 6);
        quarter(&mut x, 15, 3, 7, 11);
        quarter(&mut x, 0, 1, 2, 3);
        quarter(&mut x, 5, 6, 7, 4);
        quarter(&mut x, 10, 11, 8, 9);
        quarter(&mut x, 15, 12, 13, 14);
    }
    let mut out = [0u8; 64];
    for (i, word) in x.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.wrapping_add(state[i]).to_le_bytes());
    }
    out
}

/// Decode LZ4 data of `size` bytes in sub-blocks of `block` bytes each (the
/// last may be smaller), appended to `out`: the block format (a token a
/// sequence, its literals, a match), no frame — the game's files carry many
/// sub-blocks, so where each one ends is tracked by the decode itself
/// (wowdev.wiki/BLTE).
fn decode_lz4(out: &mut Vec<u8>, data: &[u8], size: usize, block: usize) -> Result<()> {
    let start = out.len();
    let mut at = 0;
    while out.len() - start < size {
        let sub = block.min(size - (out.len() - start));
        let used = decode_lz4_block(&data[at..], out, start, sub)
            .with_context(|| format!("an LZ4 block at {at} of a stream of {size}"))?;
        if used == 0 {
            bail!("an LZ4 stream cut short at {at}");
        }
        at += used;
    }
    if out.len() - start != size {
        bail!("an LZ4 stream came out the wrong size");
    }
    Ok(())
}

/// One LZ4 sub-block: as many sequences as fill `sub` bytes of `out` from its
/// `start`. Where the sub-block's data ends.
fn decode_lz4_block(data: &[u8], out: &mut Vec<u8>, start: usize, sub: usize) -> Result<usize> {
    let mut at = 0;
    loop {
        // A sequence: its token (the literals' and the match's lengths), the
        // literals, the match's offset. The match is copied from what the
        // stream already decoded.
        let token = *data.get(at).context("an LZ4 sequence cut short")?;
        at += 1;
        let mut literals = usize::from(token >> 4);
        if literals == 15 {
            loop {
                let byte = *data.get(at).context("an LZ4 length cut short")?;
                at += 1;
                literals += usize::from(byte);
                if byte != 255 {
                    break;
                }
            }
        }
        let literals = data.get(at..at + literals).context("an LZ4 literals cut short")?;
        out.extend_from_slice(literals);
        at += literals.len();
        if out.len() - start >= sub {
            return Ok(at); // the last sequence carries no match
        }
        let offset = usize::from(u16::from_le_bytes(
            data.get(at..at + 2)
                .context("an LZ4 match offset cut short")?
                .try_into()?,
        ));
        at += 2;
        if offset == 0 || offset > out.len() {
            bail!("an LZ4 match out of the stream");
        }
        let mut length = 4 + usize::from(token & 0x0f);
        if (token & 0x0f) == 15 {
            loop {
                let byte = *data.get(at).context("an LZ4 length cut short")?;
                at += 1;
                length += usize::from(byte);
                if byte != 255 {
                    break;
                }
            }
        }
        // The match may overlap itself: a byte at a time, from the decoded
        // bytes themselves.
        let mut from = out.len() - offset;
        while length > 0 && out.len() - start < sub {
            let byte = out[from];
            out.push(byte);
            from += 1;
            length -= 1;
        }
        if out.len() - start >= sub {
            return Ok(at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// The das-labor verified vector of the 128-bit variant ("expand 16-byte
    /// k"): key 0x80..00, IV zero, zero bytes in — the variant the game's
    /// blocks use (its sixteen-byte key doubled).
    #[test]
    fn vector_128() {
        let key: [u8; 16] = [0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let nonce = [0u8; 8];
        let stream = salsa20(&key, nonce, &[0u8; 16]);
        assert_eq!(stream[..8], [0x4D, 0xFA, 0x5E, 0x48, 0x1D, 0xA2, 0x3E, 0xA0]);
    }

    /// The same vector of the 256-bit variant ("expand 32-byte k"), of the
    /// block function itself: the state's words, not `salsa20` (whose key is
    /// the sixteen bytes the keyrings give). The key: 0x80 and thirty-one
    /// zeros, its first half as four little-endian words.
    #[test]
    fn vector_256() {
        let state = [
            0x6170_7865,
            0x0000_0080,
            0,
            0,
            0,
            0x3320_646e,
            0,
            0,
            0,
            0,
            0x7962_2d32,
            0,
            0,
            0,
            0,
            0x6b20_6574,
        ];
        let stream = block_words(&state);
        assert_eq!(stream[..8], [0xE3, 0xBE, 0x8F, 0xDD, 0x8B, 0xEC, 0xA2, 0xE3]);
    }

    /// A BLTE file of plain blocks decodes to its content.
    #[test]
    fn plain() {
        let mut file = b"BLTE".to_vec();
        file.extend_from_slice(&36u32.to_be_bytes()); // the header's own size
        file.push(0x0F); // the table's format
        file.extend_from_slice(&[0, 0, 1]); // one block
        file.extend_from_slice(&4u32.to_be_bytes()); // its compressed size
        file.extend_from_slice(&3u32.to_be_bytes()); // its decompressed size
        file.extend_from_slice(&[0; 16]); // its hash (not verified)
        file.extend_from_slice(b"Nabc"); // the block itself
        assert_eq!(decode(&file, &Keys::new()).unwrap(), b"abc");
    }

    /// A BLTE file of a zlib block decodes to its content.
    #[test]
    fn zlib() {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(b"abc").unwrap();
        let block = z.finish().unwrap();
        let mut file = b"BLTE".to_vec();
        file.extend_from_slice(&36u32.to_be_bytes());
        file.push(0x0F);
        file.extend_from_slice(&[0, 0, 1]);
        file.extend_from_slice(&(1 + block.len() as u32).to_be_bytes());
        file.extend_from_slice(&3u32.to_be_bytes());
        file.extend_from_slice(&[0; 16]);
        file.push(b'Z');
        file.extend_from_slice(&block);
        assert_eq!(decode(&file, &Keys::new()).unwrap(), b"abc");
    }

    /// An LZ4 stream decodes: literals, then a match over the literals.
    #[test]
    fn lz4() {
        // A token of 3 literals and a 4-byte match of offset 3, then the
        // match again as one more byte of its own output.
        let mut out = Vec::new();
        decode_lz4(&mut out, &[0x30, b'a', b'b', b'c', 3, 0], 7, 16).unwrap();
        assert_eq!(out, b"abcabca");
    }
}
