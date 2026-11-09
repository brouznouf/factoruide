//! The game client's local CASC storage: `Data/data` (its archives and local
//! indices), `Data/config` (the build configs). The files are found through
//! the build config of the installed product: the encoding table maps the
//! content hash of a file to the encoding hash the local indices know it by,
//! and the root manifest maps the paths (and the file data IDs) to the content
//! hashes. See NOTICE.md for the sources.

pub mod blp;
pub mod blte;
pub mod dbc;
pub mod encoding;
pub mod hotfix;
pub mod idx;
pub mod jenkins;
pub mod root;

use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// How many bytes the archive wraps every file in before its BLTE data: its
/// encoding key in reversed byte order, the size, checksums.
const ARCHIVE_HEADER: u32 = 30;

/// The file data ID of the client's `TactKey` table (the listfile's number,
/// used when the manifest holds it by ID only).
const TACT_KEY_FDID: u32 = 1_302_850;
/// The file data ID of the client's `TactKeyLookup` table.
const TACT_LOOKUP_FDID: u32 = 1_302_851;

/// The installed game, one product of it (`wowf`, `wow_classic`...).
#[derive(Debug)]
pub struct Storage {
    /// The install (`World of Warcraft`): its products' hotfix caches sit in
    /// its product folders.
    install: PathBuf,
    /// `Data/data`: the archives and their indices.
    data: PathBuf,
    indices: idx::Indices,
    encoding: encoding::Encoding,
    root: root::Root,
    /// The keys the encrypted files decrypt with, by the name their blocks ask
    /// for: the client's own, shipped with it and pushed into it.
    keys: blte::Keys,
}

impl Storage {
    /// Open the install at `install` (`World of Warcraft`, where the game's
    /// `.build.info` is) for `product`. The install's files are read from
    /// `Data`; nothing is written.
    pub fn open(install: &Path, product: &str) -> Result<Self> {
        let build_info = std::fs::read_to_string(install.join(".build.info"))
            .with_context(|| format!("reading {}\\.build.info", install.display()))?;
        let build = build_key(&build_info, product)?;
        let config = install.join("Data").join("config");
        let build_config = std::fs::read_to_string(config.join(hashed_path(&build)?))
            .with_context(|| format!("reading the build config of {product}"))?;
        let encoding_ekey = value(&build_config, "encoding")
            .and_then(|v| v.split_whitespace().nth(1).map(str::to_owned))
            .context("the build config has no encoding table")?;
        let root_ckey = value(&build_config, "root").context("the build config has no root manifest")?;
        let data = install.join("Data").join("data");
        let indices = idx::Indices::load(&data)?;
        let mut storage = Self {
            install: install.to_path_buf(),
            data,
            indices,
            encoding: encoding::Encoding::default(),
            root: root::Root::default(),
            keys: blte::Keys::new(),
        };
        // The encoding table's own data is at its encoding hash.
        let ekey = hex16(&encoding_ekey)?;
        let table = storage.file_bytes(&ekey)?;
        storage.encoding = encoding::read(&table)?;
        // The root manifest, through the table.
        let root_ckey = hex16(&root_ckey)?;
        let manifest = storage.file_by_ckey(&root_ckey)?;
        storage.root = root::read(&manifest)?;
        // The keys the client's encrypted files decrypt with: its own, shipped
        // with it. (None in a client that encrypts nothing read here.)
        storage.keys = storage.tact_keys()?;
        Ok(storage)
    }

    /// The bytes of the file at `path` (`Interface\Icons\INV_Sword_04`), if the
    /// game has it: BLTE-decoded.
    pub fn file_by_name(&self, path: &str) -> Result<Option<Vec<u8>>> {
        let Some(ckeys) = self.root.by_name(path) else {
            return Ok(None);
        };
        let mut missing = Vec::new();
        for ckey in ckeys {
            match self.file_by_ckey(ckey) {
                Ok(bytes) => return Ok(Some(bytes)),
                Err(err) => missing.push(format!("{err}")),
            }
        }
        anyhow::bail!("the file is not in the install: {}", missing.join("; "))
    }

    /// The bytes of the file with this data ID, if the game has it.
    pub fn file_by_fdid(&self, fdid: u32) -> Result<Option<Vec<u8>>> {
        let Some(ckeys) = self.root.by_fdid(fdid) else {
            return Ok(None);
        };
        let mut missing = Vec::new();
        for ckey in ckeys {
            match self.file_by_ckey(ckey) {
                Ok(bytes) => return Ok(Some(bytes)),
                Err(err) => missing.push(format!("{err}")),
            }
        }
        anyhow::bail!("the file is not in the install: {}", missing.join("; "))
    }

    /// The bytes of the file with this content hash, if the game installed it:
    /// under whichever of its encoding hashes the install has.
    pub fn file_by_ckey(&self, ckey: &[u8; 16]) -> Result<Vec<u8>> {
        let ekeys = self
            .encoding
            .all(ckey)
            .ok_or_else(|| anyhow!("the file is not in the encoding table"))?;
        let mut missing = Vec::new();
        for ekey in ekeys {
            match self.file_bytes(ekey) {
                Ok(bytes) => return Ok(bytes),
                Err(err) => missing.push(format!("{ekey:02x?}: {err}")),
            }
        }
        bail!("the file is not in the local indices: {}", missing.join("; "))
    }

    /// How many files the root manifest holds (for a report).
    pub fn files(&self) -> usize {
        self.root.len()
    }

    /// The keys the client's encrypted files decrypt with, by the name their
    /// blocks ask for: the client's own TACT keys, from its `TactKey` table,
    /// named by its `TactKeyLookup` one (a lookup row's ID is the key's, its
    /// record the name), and the ones the server pushed into its hotfix
    /// caches. Empty when the client carries none — then its encrypted files
    /// stay closed (an error when read).
    fn tact_keys(&self) -> Result<blte::Keys> {
        let mut keys = blte::Keys::new();
        let (Some(lookup), Some(key_table)) = (
            self.database("DBFilesClient\\TactKeyLookup.db2", TACT_LOOKUP_FDID)?,
            self.database("DBFilesClient\\TactKey.db2", TACT_KEY_FDID)?,
        ) else {
            return Ok(keys);
        };
        let lookup = dbc::Table::read(&lookup)?;
        let key_table = dbc::Table::read(&key_table)?;
        // The keys by their ID, then the names by the ID they name.
        let mut by_id = HashMap::new();
        for row in 0..key_table.len() {
            let record = key_table.record(row)?;
            if record.len() == 16 {
                by_id.insert(key_table.id(row)?, record.try_into().expect("sixteen bytes"));
            }
        }
        for row in 0..lookup.len() {
            let record = lookup.record(row)?;
            if record.len() == 8
                && let Some(key) = by_id.get(&lookup.id(row)?)
            {
                keys.insert(record.try_into().expect("eight bytes"), *key);
            }
        }
        // The hotfix caches of the install's products: the keys the server
        // pushed into the client, newer than the ones it shipped with. A cache
        // of a version not read (an older client's) is skipped.
        for cache in self.hotfix_caches() {
            if let Ok(data) = std::fs::read(&cache)
                && let Ok(hotfixed) = hotfix::tact_keys(&data)
            {
                keys.extend(hotfixed);
            }
        }
        Ok(keys)
    }

    /// The hotfix caches of the install's products, wherever they sit:
    /// `<install>/<product folder>/Cache/ADB/<locale>/DBCache.bin`.
    fn hotfix_caches(&self) -> Vec<PathBuf> {
        std::fs::read_dir(&self.install)
            .into_iter()
            .flatten()
            .flatten()
            .flat_map(|product| {
                let adb = product.path().join("Cache").join("ADB");
                std::fs::read_dir(&adb).into_iter().flatten().flatten()
            })
            .flat_map(|locale| std::fs::read_dir(locale.path()).into_iter().flatten().flatten())
            .filter(|file| file.file_name().eq_ignore_ascii_case("DBCache.bin"))
            .map(|file| file.path())
            .collect()
    }

    /// The bytes of a database of the client, by its path or its file data ID
    /// (the listfile's number): whichever the manifest holds.
    fn database(&self, name: &str, fdid: u32) -> Result<Option<Vec<u8>>> {
        if let Some(bytes) = self.file_by_name(name)? {
            return Ok(Some(bytes));
        }
        self.file_by_fdid(fdid)
    }

    /// The file at `ekey` (its first bytes): BLTE-decoded.
    fn file_bytes(&self, ekey: &[u8; 16]) -> Result<Vec<u8>> {
        let place = self
            .indices
            .get(ekey)
            .ok_or_else(|| anyhow!("the file is not in the local indices"))?;
        let mut archive = std::fs::File::open(self.data.join(format!("data.{:03}", place.archive)))
            .with_context(|| format!("opening data.{:03}", place.archive))?;
        // The archive wraps every file in a header, then the BLTE data.
        if place.size <= ARCHIVE_HEADER {
            bail!("the file's entry is too short for its header");
        }
        let mut encoded = vec![0u8; (place.size - ARCHIVE_HEADER) as usize];
        archive.seek(SeekFrom::Start(u64::from(place.offset) + u64::from(ARCHIVE_HEADER)))?;
        archive.read_exact(&mut encoded)?;
        blte::decode(&encoded, &self.keys)
    }
}

/// The path of a config file inside `Data/config`, hashed in two folders:
/// `config/<b0><b1>/<b2><b3>/<whole hash>`.
fn hashed_path(hash: &str) -> Result<PathBuf> {
    let bytes = hash.as_bytes();
    if bytes.len() < 4 || !bytes[..4].iter().all(u8::is_ascii_hexdigit) {
        bail!("a build key of {hash:?}, not four hex digits at its head");
    }
    let mut out = PathBuf::from(&hash[..2]);
    out.push(&hash[2..4]);
    out.push(hash);
    Ok(out)
}

/// The build key of `product` in `.build.info` (its columns by name).
fn build_key(build_info: &str, product: &str) -> Result<String> {
    let mut lines = build_info.lines();
    let header = lines.next().context("an empty .build.info")?;
    let columns: Vec<&str> = header.split('|').collect();
    let product_at = columns
        .iter()
        .position(|c| c.strip_prefix("Product!STRING").is_some())
        .context("no Product column in .build.info")?;
    let build_at = columns
        .iter()
        .position(|c| c.strip_prefix("Build Key!HEX").is_some())
        .context("no Build Key column in .build.info")?;
    for line in lines {
        let fields: Vec<&str> = line.split('|').collect();
        if fields.get(product_at).is_some_and(|p| *p == product) {
            let key = fields.get(build_at).context("a product line without its build key")?;
            return Ok((*key).to_owned());
        }
    }
    bail!("the install has no product {product}")
}

/// The value of `key = ...` in a build config.
fn value(config: &str, key: &str) -> Option<String> {
    config
        .lines()
        .find_map(|line| line.strip_prefix(&format!("{key} = ")).map(str::to_owned))
}

/// A hex string as bytes.
fn hex(text: &str) -> Result<Vec<u8>> {
    let text = text.trim();
    if !text.len().is_multiple_of(2) {
        bail!("odd hex string");
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).with_context(|| "hex digits"))
        .collect()
}

/// A 32-character hex string as 16 bytes.
fn hex16(text: &str) -> Result<[u8; 16]> {
    let bytes = hex(text)?;
    bytes.try_into().map_err(|_| anyhow::anyhow!("not 16 bytes"))
}
