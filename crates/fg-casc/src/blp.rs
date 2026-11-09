//! BLP2, the game's own picture format: a 148-byte header, then the mipmaps as
//! JPEG, palette indices, DXT blocks or plain BGRA pixels (wowdev.wiki/BLP).
//! The first mipmap is the picture; the icons read here are paletted or DXT.

use anyhow::{Context, Result, bail};
use image::{Rgba, RgbaImage};

/// The PNG encoding of a BLP2 picture's first mipmap (the pack's format).
pub fn png(data: &[u8]) -> Result<Vec<u8>> {
    let picture = decode(data)?;
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(picture)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .context("encoding the picture as a PNG")?;
    Ok(png)
}

/// Decode the first mipmap of a BLP2 file into RGBA pixels.
pub fn decode(data: &[u8]) -> Result<RgbaImage> {
    let u32le = |at: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            data.get(at..at + 4).context("the BLP2 header cut short")?.try_into()?,
        ))
    };
    if data.len() < 0x18 || &data[..4] != b"BLP2" {
        bail!("not a BLP2 picture");
    }
    let encoding = data[8];
    let alpha_depth = data[9];
    let format = data[10];
    let width = u32le(0x0C)?;
    let height = u32le(0x10)?;
    let offset = u32le(0x14)? as usize;
    let size = u32le(0x54)? as usize;
    if width == 0 || height == 0 || width > 8192 || height > 8192 {
        bail!("BLP2 of {width}x{height} pixels, out of bounds");
    }
    if offset + size > data.len() {
        bail!("BLP2 mipmap out of bounds");
    }
    let mipmap = &data[offset..offset + size];
    let (w, h) = (width as usize, height as usize);
    let mut out = RgbaImage::new(width, height);
    match encoding {
        // Plain BGRA pixels.
        3 => {
            if mipmap.len() < w * h * 4 {
                bail!("BLP2 pixels cut short");
            }
            for y in 0..h {
                for x in 0..w {
                    let at = (y * w + x) * 4;
                    let [b, g, r, a] = mipmap[at..at + 4].try_into().expect("four bytes");
                    out.put_pixel(x as u32, y as u32, Rgba([r, g, b, a]));
                }
            }
        }
        // Palette indices, the alpha after them (none, a bit a pixel, or a
        // byte a pixel).
        1 => {
            let alpha_len = match alpha_depth {
                0 => 0,
                1 => (w * h).div_ceil(8),
                _ => w * h,
            };
            if mipmap.len() < w * h + alpha_len {
                bail!("BLP2 indices cut short");
            }
            let alpha = &mipmap[w * h..];
            for y in 0..h {
                for x in 0..w {
                    let index = mipmap[y * w + x] as usize;
                    let entry = u32le(0x94 + index * 4)?;
                    let [b, g, r, _] = entry.to_le_bytes();
                    let a = match alpha_depth {
                        0 => u8::MAX,
                        1 => u8::from(alpha[(y * w + x) / 8] & 1 << (x % 8) != 0) * u8::MAX,
                        _ => alpha[y * w + x],
                    };
                    out.put_pixel(x as u32, y as u32, Rgba([r, g, b, a]));
                }
            }
        }
        // DXT blocks: DXT1 (8 bytes, three colors and a transparent black when
        // the first color word is the lesser), DXT3 (explicit 4-bit alphas),
        // DXT5 (two alphas interpolated).
        2 => {
            let dxt1 = format == 0;
            let block = if dxt1 { 8 } else { 16 };
            let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
            if mipmap.len() < bw * bh * block {
                bail!("BLP2 DXT blocks cut short");
            }
            let alpha = |at: usize| -> Option<[u8; 16]> {
                match format {
                    1 => Some(dxt3_alpha(mipmap, at)),
                    7 => Some(dxt5_alpha(mipmap, at)),
                    _ => None,
                }
            };
            for by in 0..bh {
                for bx in 0..bw {
                    let at = (by * bw + bx) * block;
                    // The colors after the alphas (at the block's start in DXT1).
                    let color_at = at + block - 8;
                    let colors = dxt_colors(
                        u16::from_le_bytes(mipmap[color_at..color_at + 2].try_into().expect("two bytes")),
                        u16::from_le_bytes(mipmap[color_at + 2..color_at + 4].try_into().expect("two bytes")),
                        dxt1,
                    );
                    let indices =
                        u32::from_le_bytes(mipmap[color_at + 4..color_at + 8].try_into().expect("four bytes"));
                    let alpha = alpha(at);
                    for py in 0..4 {
                        for px in 0..4 {
                            let (x, y) = (bx * 4 + px, by * 4 + py);
                            if x >= w || y >= h {
                                continue;
                            }
                            let which = py * 4 + px;
                            let [r, g, b] = colors[((indices >> (which * 2)) & 3) as usize];
                            let a = alpha.map_or(u8::MAX, |a| a[which]);
                            out.put_pixel(x as u32, y as u32, Rgba([r, g, b, a]));
                        }
                    }
                }
            }
        }
        other => bail!("BLP2 encoding {other} (JPEG) is not decoded"),
    }
    Ok(out)
}

/// The four colors of a DXT block: its two 16-bit ones and the two interpolated
/// between them. Three colors and a transparent black when `dxt1` and the first
/// color word is the lesser (the one-bit-alpha blocks).
fn dxt_colors(a: u16, b: u16, dxt1: bool) -> [[u8; 3]; 4] {
    let color = |word: u16| {
        [
            ((word >> 11 & 0x1f) * 0xff / 0x1f) as u8,
            ((word >> 5 & 0x3f) * 0xff / 0x3f) as u8,
            ((word & 0x1f) * 0xff / 0x1f) as u8,
        ]
    };
    let mix = |a: [u8; 3], b: [u8; 3], wa: u16| {
        std::array::from_fn(|k| ((u16::from(a[k]) * wa + u16::from(b[k]) * (3 - wa)) / 3) as u8)
    };
    let (ca, cb) = (color(a), color(b));
    if dxt1 && a <= b {
        [ca, cb, mix(ca, cb, 2), [0, 0, 0]]
    } else {
        [ca, cb, mix(ca, cb, 2), mix(ca, cb, 1)]
    }
}

/// The 16 alphas of a DXT3 block: 4 bits each, in its first 8 bytes.
fn dxt3_alpha(mipmap: &[u8], at: usize) -> [u8; 16] {
    let mut alpha = [0u8; 16];
    for (k, byte) in mipmap[at..at + 8].iter().enumerate() {
        // Two alphas a byte, the low nibble first, each spread to eight bits.
        alpha[k * 2] = (byte & 0x0f) * 0x11;
        alpha[k * 2 + 1] = (byte >> 4) * 0x11;
    }
    alpha
}

/// The 16 alphas of a DXT5 block: two 8-bit ones, interpolated, in its first 8
/// bytes (16 bits of them skipped).
fn dxt5_alpha(mipmap: &[u8], at: usize) -> [u8; 16] {
    let (a0, a1) = (mipmap[at], mipmap[at + 1]);
    let mut alpha = [0u8; 16];
    let bits = u64::from_le_bytes(mipmap[at..at + 8].try_into().expect("eight bytes")) >> 16;
    let table: [u8; 8] = if a0 > a1 {
        [
            a0,
            a1,
            ((u16::from(a0) * 6 + u16::from(a1)) / 7) as u8,
            ((u16::from(a0) * 5 + u16::from(a1) * 2) / 7) as u8,
            ((u16::from(a0) * 4 + u16::from(a1) * 3) / 7) as u8,
            ((u16::from(a0) * 3 + u16::from(a1) * 4) / 7) as u8,
            ((u16::from(a0) * 2 + u16::from(a1) * 5) / 7) as u8,
            ((u16::from(a0) + u16::from(a1) * 6) / 7) as u8,
        ]
    } else {
        [
            a0,
            a1,
            ((u16::from(a0) * 4 + u16::from(a1)) / 5) as u8,
            ((u16::from(a0) * 3 + u16::from(a1) * 2) / 5) as u8,
            ((u16::from(a0) * 2 + u16::from(a1) * 3) / 5) as u8,
            ((u16::from(a0) + u16::from(a1) * 4) / 5) as u8,
            0,
            u8::MAX,
        ]
    };
    for (k, a) in alpha.iter_mut().enumerate() {
        *a = table[((bits >> (k * 3)) & 7) as usize];
    }
    alpha
}
