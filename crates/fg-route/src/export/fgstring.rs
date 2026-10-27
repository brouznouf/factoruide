//! The texts copied between the web app and the addon: a prefix telling what it holds, then
//! the content compressed (raw DEFLATE) and written with LibDeflate's printable alphabet
//! (`EncodeForPrint`), which the addon decodes with LibDeflate.

/// A guide import (web app -> game): the Lua the app used to write in the addon's files.
pub const GUIDE_PREFIX: &str = "FGG1:";
/// A character's export (game -> web app): its profile and recordings, as JSON.
pub const PROFILE_PREFIX: &str = "FGP1:";

/// Exports bigger than this once decompressed are refused (a profile with its recordings
/// takes a few megabytes at most).
const MAX_DECODED: usize = 64 << 20;

const ALPHABET: &[u8; 64] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789()";

/// `prefix` and `data` compressed and encoded.
pub fn encode(prefix: &str, data: &[u8]) -> String {
    let packed = miniz_oxide::deflate::compress_to_vec(data, 9);
    let mut out = String::with_capacity(prefix.len() + packed.len() * 4 / 3 + 4);
    out.push_str(prefix);
    encode_for_print(&packed, &mut out);
    out
}

/// What a text made by `encode` with `prefix` holds.
pub fn decode(prefix: &str, text: &str) -> Result<Vec<u8>, String> {
    let text = text.trim();
    let body = text.strip_prefix(prefix).ok_or("not_this_kind")?;
    let packed = decode_for_print(body).ok_or("corrupt")?;
    miniz_oxide::inflate::decompress_to_vec_with_limit(&packed, MAX_DECODED).map_err(|_| "corrupt".to_owned())
}

/// What a character's export holds (`/fg export`, see the addon's Profile.lua).
#[derive(Debug, serde::Deserialize)]
pub struct ProfileExport {
    format: String,
    version: u32,
    /// Key of the game version (forever, classic, tbc); none from older addons.
    pub edition: Option<String>,
    /// The addon's version.
    #[serde(default)]
    pub addon: Option<String>,
    /// "Name-Realm".
    pub character: String,
    /// The profile, named after `character` when it has no name.
    pub profile: serde_json::Value,
}

/// A character's export pasted. Errors: `not_a_profile` (another text), `corrupt_profile`
/// (cut or damaged).
pub fn read_profile(text: &str) -> Result<ProfileExport, String> {
    let json = decode(PROFILE_PREFIX, text).map_err(|e| {
        if e == "not_this_kind" {
            "not_a_profile"
        } else {
            "corrupt_profile"
        }
        .to_owned()
    })?;
    let mut export: ProfileExport = serde_json::from_slice(&json).map_err(|_| "corrupt_profile".to_owned())?;
    if export.format != "factoruide-profile" || export.version != 1 {
        return Err("not_a_profile".into());
    }
    export.character = export.character.trim().to_owned();
    if export.character.is_empty() || export.character.len() > 100 || !export.profile.is_object() {
        return Err("corrupt_profile".into());
    }
    if export.profile["name"].as_str().is_none_or(str::is_empty) {
        export.profile["name"] = export.character.clone().into();
    }
    Ok(export)
}

/// LibDeflate's `EncodeForPrint`: 3 bytes as 4 characters of 6 bits, little-endian, the low
/// bits first; the last 1 or 2 bytes as just enough characters.
fn encode_for_print(data: &[u8], out: &mut String) {
    let char_of = |v: u32| char::from(ALPHABET[(v & 63) as usize]);
    let (chunks, rest) = data.as_chunks::<3>();
    for c in chunks {
        let v = u32::from(c[0]) | u32::from(c[1]) << 8 | u32::from(c[2]) << 16;
        for shift in [0, 6, 12, 18] {
            out.push(char_of(v >> shift));
        }
    }
    let mut cache = 0u32;
    let mut bits = 0i32;
    for (i, &b) in rest.iter().enumerate() {
        cache |= u32::from(b) << (8 * i);
        bits += 8;
    }
    while bits > 0 {
        out.push(char_of(cache));
        cache >>= 6;
        bits -= 6;
    }
}

/// LibDeflate's `DecodeForPrint` (None: a character not of the alphabet).
fn decode_for_print(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| ALPHABET.iter().position(|&a| a == c).map(|v| v as u32);
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4 + 2);
    let (chunks, rest) = bytes.as_chunks::<4>();
    for c in chunks {
        let v = value(c[0])? | value(c[1])? << 6 | value(c[2])? << 12 | value(c[3])? << 18;
        out.extend_from_slice(&[v as u8, (v >> 8) as u8, (v >> 16) as u8]);
    }
    let mut cache = 0u32;
    let mut bits = 0;
    for &c in rest {
        cache |= value(c)? << bits;
        bits += 6;
        while bits >= 8 {
            out.push(cache as u8);
            cache >>= 8;
            bits -= 8;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Whatever the app packs comes back as it was, under the same prefix only.
        #[test]
        fn what_is_encoded_is_decoded(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
            let text = encode(GUIDE_PREFIX, &data);
            prop_assert_eq!(decode(GUIDE_PREFIX, &text), Ok(data));
            prop_assert_eq!(decode(PROFILE_PREFIX, &text), Err("not_this_kind".to_owned()));
        }

        /// Text pasted by a player never makes the decoder panic.
        #[test]
        fn any_text_is_refused_not_a_panic(text in ".{0,512}") {
            let _ = decode(GUIDE_PREFIX, &text);
            let _ = decode(GUIDE_PREFIX, &format!("{GUIDE_PREFIX}{text}"));
        }
    }
    use std::fmt::Write as _;

    #[test]
    fn what_is_encoded_decodes_whatever_its_length() {
        for len in 0..40 {
            let data: Vec<u8> = (0..len).map(|i| (i * 37 % 256) as u8).collect();
            let text = encode(GUIDE_PREFIX, &data);
            assert!(text[GUIDE_PREFIX.len()..].bytes().all(|b| ALPHABET.contains(&b)));
            assert_eq!(decode(GUIDE_PREFIX, &format!("  {text}\n")).unwrap(), data);
        }
    }

    /// The printable form of LibDeflate: "abc" is 3 bytes, 4 characters.
    #[test]
    fn printable_form_matches_libdeflate() {
        let mut out = String::new();
        encode_for_print(b"\x00\x00\x00", &mut out);
        assert_eq!(out, "aaaa");
        out.clear();
        encode_for_print(b"\xff\xff\xff", &mut out);
        assert_eq!(out, "))))");
        out.clear();
        encode_for_print(b"\x01", &mut out);
        assert_eq!(out, "ba");
        assert_eq!(decode_for_print("ba").unwrap(), b"\x01");
    }

    /// The addon's LibDeflate reads what is encoded here and the other way round (with a Lua
    /// interpreter at hand: luajit or lua5.1).
    #[test]
    fn the_addon_reads_them_and_they_read_the_addon() {
        let Some(lua) = ["luajit", "lua5.1"]
            .into_iter()
            .find(|l| std::process::Command::new(l).arg("-v").output().is_ok())
        else {
            return;
        };
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../addon/tests/fgstring.lua");
        let dir = std::env::temp_dir().join(format!("fg-string-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let text = (0..3000).fold(String::new(), |mut t, i| {
            let _ = writeln!(t, "FG.Routes[{i}] = {{ t = \"é{i}\" }}");
            t
        });
        let run = |args: &[&std::path::Path]| {
            let out = std::process::Command::new(lua).arg(script).args(args).output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8(out.stdout).unwrap()
        };
        let encoded = dir.join("encoded.txt");
        std::fs::write(&encoded, encode(GUIDE_PREFIX, text.as_bytes())).unwrap();
        assert_eq!(run(&[std::path::Path::new("decode"), &encoded]), text);
        let plain = dir.join("plain.json");
        std::fs::write(&plain, &text).unwrap();
        let theirs = run(&[std::path::Path::new("encode-profile"), &plain]);
        assert_eq!(decode(PROFILE_PREFIX, &theirs).unwrap(), text.as_bytes());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn another_kind_or_a_damaged_text_is_refused() {
        let text = encode(PROFILE_PREFIX, b"{}");
        assert_eq!(decode(GUIDE_PREFIX, &text).unwrap_err(), "not_this_kind");
        assert_eq!(decode(PROFILE_PREFIX, &format!("{text}!")).unwrap_err(), "corrupt");
        assert_eq!(decode(PROFILE_PREFIX, "FGP1:abcdabcd").unwrap_err(), "corrupt");
    }
}
