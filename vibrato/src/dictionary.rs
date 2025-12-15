//! Dictionary for tokenization.
pub mod builder;
pub(crate) mod character;
pub(crate) mod config;
pub(crate) mod connector;
pub(crate) mod fetch;
pub(crate) mod lexicon;
pub(crate) mod mapper;
pub(crate) mod unknown;
pub(crate) mod word_idx;

use std::fs::{self, File, Metadata, create_dir_all};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::ops::Deref;

use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use memmap2::Mmap;
use rkyv::{
    access, access_unchecked, api::serialize_using, ser::allocator::Arena, ser::sharing::Share,
    Archive, Archived, Deserialize, Serialize, rancor::Error,
    ser::writer::IoWriter, ser::Serializer, util::{with_arena, AlignedVec},
};
use sha2::{Digest, Sha256};

use crate::dictionary::character::{ArchivedCharProperty, CharProperty};
use crate::dictionary::connector::{ArchivedConnectorWrapper, Connector, ConnectorWrapper};
use crate::dictionary::lexicon::{ArchivedLexicon, Lexicon};
use crate::dictionary::mapper::ConnIdMapper;
use crate::dictionary::unknown::{ArchivedUnkHandler, UnkHandler};
use crate::errors::{Result, VibratoError};

pub use crate::dictionary::builder::SystemDictionaryBuilder;
pub use crate::dictionary::word_idx::WordIdx;

pub(crate) use crate::dictionary::lexicon::WordParam;

#[cfg(feature = "download")]
pub use crate::dictionary::config::PresetDictionaryKind;

/// Magic bytes identifying Vibrato Tokenizer.
///
/// The version "0.6" in this constant indicates the model format version, which is
/// now decoupled from the crate's semantic version. This magic byte
/// is currently not expected to be modified. This is based on the policy of maintaining
/// backward compatibility with dictionary formats.
pub const MODEL_MAGIC: &[u8] = b"VibratoTokenizerRkyv 0.6\n";

const MODEL_MAGIC_LEN: usize = MODEL_MAGIC.len();
const RKYV_ALIGNMENT: usize = 16;
const PADDING_LEN: usize = (RKYV_ALIGNMENT - (MODEL_MAGIC_LEN % RKYV_ALIGNMENT)) % RKYV_ALIGNMENT;
const DATA_START: usize = MODEL_MAGIC_LEN + PADDING_LEN;

/// Prefix of magic bytes for legacy bincode-based models.
pub const LEGACY_MODEL_MAGIC_PREFIX: &[u8] = b"VibratoTokenizer 0.";

pub static GLOBAL_CACHE_DIR: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
    let path = dirs::cache_dir()?.join("vibrato-rkyv");
    fs::create_dir_all(&path).ok()?;

    Some(path)
});

pub static GLOBAL_DATA_DIR: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
    let path = dirs::data_local_dir()?.join("vibrato-rkyv");
    fs::create_dir_all(&path).ok()?;

    Some(path)
});

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum LoadMode {
    /// Perform validation on every load. (Safest)
    Validate,
    /// Skip validation if a pre-computed hash matches. (Fastest for repeated loads)
    TrustCache,
}

/// Specifies the caching strategy for dictionaries decompressed from a Zstandard archive.
pub enum CacheStrategy {
    /// Creates a `.cache` subdirectory in the same directory as the compressed dictionary.
    ///
    /// This strategy keeps the cache data alongside the original file.
    /// Fails if the parent directory is not writable.
    Local,

    /// Uses a shared, user-specific cache directory appropriate for the operating system.
    ///
    /// This is a good default for most applications, especially when dictionary files might
    /// be stored in read-only locations. The path is determined by `dirs::cache_dir()`.
    ///
    /// | Platform | Value                             | Example                               |
    /// | -------- | --------------------------------- | ------------------------------------- |
    /// | Linux    | `$XDG_CACHE_HOME` or `$HOME/.cache` | `/home/alice/.cache`                  |
    /// | macOS    | `$HOME/Library/Caches`            | `/Users/Alice/Library/Caches`         |
    /// | Windows  | `{FOLDERID_LocalAppData}`         | `C:\Users\Alice\AppData\Local`        |
    ///
    GlobalCache,

    /// Uses a shared, user-specific data directory appropriate for the operating system.
    ///
    /// This is similar to `GlobalCache` but uses a directory typically intended for persistent,
    /// non-roaming application data. The path is determined by `dirs::data_local_dir()`.
    ///
    /// | Platform | Value                                     | Example                               |
    /// | -------- | ----------------------------------------- | ------------------------------------- |
    /// | Linux    | `$XDG_DATA_HOME` or `$HOME/.local/share`  | `/home/alice/.local/share`            |
    /// | macOS    | `$HOME/Library/Application Support`       | `/Users/Alice/Library/Application Support` |
    /// | Windows  | `{FOLDERID_LocalAppData}`                 | `C:\Users\Alice\AppData\Local`        |
    ///
    GlobalData,
}

/// Inner data of [`Dictionary`].
#[derive(Archive, Serialize, Deserialize)]
pub struct DictionaryInner {
    system_lexicon: Lexicon,
    user_lexicon: Option<Lexicon>,
    connector: ConnectorWrapper,
    mapper: Option<ConnIdMapper>,
    char_prop: CharProperty,
    unk_handler: UnkHandler,
}

// Wrapper to own the memory buffer (mmap or heap) and provide access to the archived dictionary.
#[allow(dead_code)]
enum DictBuffer {
    Mmap(Mmap),
    Aligned(AlignedVec<16>),
}

/// A read-only dictionary for tokenization, loaded via zero-copy deserialization.
pub enum Dictionary {
    Archived(ArchivedDictionary),
    Owned {
        dict: Arc<DictionaryInner>,
        _caching_handle: Option<Arc<std::thread::JoinHandle<Result<()>>>>,
    },
}

pub struct ArchivedDictionary {
    _buffer: DictBuffer,
    data: &'static ArchivedDictionaryInner,
}

pub(crate) enum DictionaryInnerRef<'a> {
    Archived(&'a ArchivedDictionaryInner),
    Owned(&'a DictionaryInner),
}

pub(crate) enum ConnectorKindRef<'a> {
    Archived(&'a ArchivedConnectorWrapper),
    Owned(&'a ConnectorWrapper),
}

impl Deref for ArchivedDictionary {
    type Target = ArchivedDictionaryInner;
    fn deref(&self) -> &Self::Target {
        self.data
    }
}

/// Type of a lexicon that contains the word.
#[derive(
    Clone, Copy, Eq, PartialEq, Debug, Hash,
    Archive, Serialize, Deserialize,
)]
#[rkyv(
    compare(PartialEq),
    derive(Debug, Eq, PartialEq, Hash, Clone, Copy),
)]
#[repr(u8)]
#[derive(Default)]
pub enum LexType {
    /// System lexicon.
    #[default]
    System,
    /// User lexicon.
    User,
    /// Unknown words.
    Unknown,
}

impl ArchivedLexType {
    /// Converts this [`ArchivedLexType`] into its corresponding [`LexType`].
    pub fn to_native(&self) -> LexType {
        match self {
            ArchivedLexType::System => LexType::System,
            ArchivedLexType::User => LexType::User,
            ArchivedLexType::Unknown => LexType::Unknown,
        }
    }
}

impl Drop for Dictionary {
    fn drop(&mut self) {
        if let Dictionary::Owned { _caching_handle, .. } = self
            && let Some(handle_arc) = _caching_handle.take()
            && let Ok(handle) = Arc::try_unwrap(handle_arc)
            && let Err(e) = handle.join() {
                log::error!("[vibrato-rkyv] Background caching thread panicked: {:?}", e);
            }
    }
}

impl DictionaryInner {
    /// Gets the reference to the system lexicon.
    #[inline(always)]
    pub(crate) const fn system_lexicon(&self) -> &Lexicon {
        &self.system_lexicon
    }

    /// Gets the reference to the user lexicon.
    #[inline(always)]
    pub(crate) const fn user_lexicon(&self) -> Option<&Lexicon> {
        self.user_lexicon.as_ref()
    }

    /// Gets the reference to the mapper for connection ids.
    #[allow(dead_code)]
    #[inline(always)]
    pub(crate) const fn mapper(&self) -> Option<&ConnIdMapper> {
        self.mapper.as_ref()
    }

    /// Gets the reference to the character property.
    #[inline(always)]
    pub(crate) const fn char_prop(&self) -> &CharProperty {
        &self.char_prop
    }

    /// Gets the reference to the handler of unknown words.
    #[inline(always)]
    pub(crate) const fn unk_handler(&self) -> &UnkHandler {
        &self.unk_handler
    }

    /// Gets the reference to the feature string.
    #[inline(always)]
    pub fn word_feature(&self, word_idx: WordIdx) -> &str {
        match word_idx.lex_type {
            LexType::System => self.system_lexicon().word_feature(word_idx),
            LexType::User => self.user_lexicon().unwrap().word_feature(word_idx),
            LexType::Unknown => self.unk_handler().word_feature(word_idx),
        }
    }

    pub(crate) fn connector(&self) -> &ConnectorWrapper {
        &self.connector
    }

    /// Gets the word parameter.
    #[inline(always)]
    pub(crate) fn word_param(&self, word_idx: WordIdx) -> WordParam {
        match word_idx.lex_type {
            LexType::System => self.system_lexicon().word_param(word_idx),
            LexType::User => self.user_lexicon().as_ref().unwrap().word_param(word_idx),
            LexType::Unknown => self.unk_handler().word_param(word_idx),
        }
    }

    /// Serializes the dictionary data to a writer using the `rkyv` format.
    ///
    /// The output binary from this function is the format that `vibrato-rkyv`'s
    /// loading methods, such as `Dictionary::from_path`, expect.
    ///
    /// # Examples
    ///
    /// This example shows how to build a dictionary from CSV data in memory
    /// and write the serialized binary to a file.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use std::fs::File;
    /// use std::io::Cursor;
    /// use vibrato_rkyv::dictionary::SystemDictionaryBuilder;
    ///
    /// // Create a dictionary instance with a builder from some source data.
    /// let dict = SystemDictionaryBuilder::from_readers(
    ///     Cursor::new("東京,名詞,地名\n"),
    ///     Cursor::new("1 1 0\n"),
    ///     Cursor::new("DEFAULT 0 0 0\n"),
    ///     Cursor::new("DEFAULT,5,5,-1000\n"),
    /// )?;
    ///
    /// // Serialize the dictionary to a file.
    /// let mut file = File::create("system.dic")?;
    /// dict.write(&mut file)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Writing to the underlying `writer` fails (e.g., an I/O error).
    /// - The `rkyv` serialization process encounters an error.
    pub fn write<W>(&self, mut wtr: W) -> Result<()>
    where
        W: Write,
    {
        wtr.write_all(MODEL_MAGIC)?;

        let padding_bytes = vec![0xFF; PADDING_LEN];
        wtr.write_all(&padding_bytes)?;

        with_arena(|arena: &mut Arena| {
            let writer = IoWriter::new(&mut wtr);
            let mut serializer = Serializer::new(writer, arena.acquire(), Share::new());
            serialize_using::<_, rkyv::rancor::Error>(self, &mut serializer)
        })
        .map_err(|e| {
            VibratoError::invalid_state("rkyv serialization failed".to_string(), e.to_string())
        })?;

        Ok(())
    }

    /// Resets the user dictionary from a reader.
    /// This should be called before serializing the dictionary.
    pub fn reset_user_lexicon_from_reader<R>(mut self, user_lexicon_rdr: Option<R>) -> Result<Self>
    where
        R: Read,
    {
        if let Some(user_lexicon_rdr) = user_lexicon_rdr {
            let mut user_lexicon = Lexicon::from_reader(user_lexicon_rdr, LexType::User)?;
            if let Some(mapper) = self.mapper.as_ref() {
                user_lexicon.map_connection_ids(mapper);
            }
            if !user_lexicon.verify(&self.connector) {
                return Err(VibratoError::invalid_argument(
                    "user_lexicon_rdr",
                    "includes invalid connection ids.",
                ));
            }
            self.user_lexicon = Some(user_lexicon);
        } else {
            self.user_lexicon = None;
        }
        Ok(self)
    }

    /// Edits connection ids with the given mappings.
    /// This should be called before serializing the dictionary.
    pub fn map_connection_ids_from_iter<L, R>(mut self, lmap: L, rmap: R) -> Result<Self>
    where
        L: IntoIterator<Item = u16>,
        R: IntoIterator<Item = u16>,
    {
        let mapper = ConnIdMapper::from_iter(lmap, rmap)?;
        self.system_lexicon.map_connection_ids(&mapper);
        if let Some(user_lexicon) = self.user_lexicon.as_mut() {
            user_lexicon.map_connection_ids(&mapper);
        }
        self.connector.map_connection_ids(&mapper);
        self.unk_handler.map_connection_ids(&mapper);
        self.mapper = Some(mapper);
        Ok(self)
    }
}

impl Dictionary {
    /// Creates a dictionary from `DictionaryInner`.
    pub fn from_inner(dict: DictionaryInner) -> Self {
        Self::Owned{ dict: Arc::new(dict), _caching_handle: None }
    }

    /// Serializes the dictionary data to a writer using the `rkyv` format.
    ///
    /// The output binary from this function is the format that `vibrato-rkyv`'s
    /// loading methods, such as `Dictionary::from_path`, expect.
    ///
    /// # Examples
    ///
    /// This example shows how to build a dictionary from CSV data in memory
    /// and write the serialized binary to a file.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use std::fs::File;
    /// use std::io::Cursor;
    /// use vibrato_rkyv::{Dictionary, SystemDictionaryBuilder};
    ///
    /// // Create a dictionary instance with a builder from some source data.
    /// let dict = SystemDictionaryBuilder::from_readers(
    ///     Cursor::new("東京,名詞,地名\n"),
    ///     Cursor::new("1 1 0\n"),
    ///     Cursor::new("DEFAULT 0 0 0\n"),
    ///     Cursor::new("DEFAULT,5,5,-1000\n"),
    /// )?;
    ///
    /// let dict = Dictionary::from_inner(dict);
    ///
    /// // Serialize the dictionary to a file.
    /// let mut file = File::create("system.dic")?;
    /// dict.write(&mut file)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Writing to the underlying `writer` fails (e.g., an I/O error).
    /// - The `rkyv` serialization process encounters an error.
    ///
    /// # Panics
    ///
    /// Panics if this method is called on a `Dictionary::Archived` variant.
    pub fn write<W>(&self, wtr: W) -> Result<()>
    where
        W: Write,
    {
        match self {
            Dictionary::Owned { dict, ..} => dict.write(wtr),
            Dictionary::Archived(_) => unreachable!(),
        }
    }


    /// Creates a dictionary from a reader by loading all data into a heap buffer.
    ///
    /// This is a fallback for when a file path is not available (e.g., reading from an
    /// in-memory buffer). It is less memory-efficient than `from_path` as it reads
    /// the entire content into memory.
    ///
    /// # Arguments
    ///
    /// * `rdr` - A reader that implements `std::io::Read`.
    ///
    /// # Errors
    ///
    /// Returns an error if the data cannot be read or if its contents are invalid.
    pub fn read<R: Read>(mut rdr: R) -> Result<Self> {
        let mut magic = [0; MODEL_MAGIC_LEN];
        rdr.read_exact(&mut magic)?;

        if magic.starts_with(LEGACY_MODEL_MAGIC_PREFIX) {
            return Err(VibratoError::invalid_argument(
                "rdr",
                "This appears to be a legacy bincode-based dictionary file. Please use a dictionary compiled for the rkyv version of vibrato.",
            ));
        }else if !magic.starts_with(MODEL_MAGIC) {
            return Err(VibratoError::invalid_argument(
                "rdr",
                "The magic number of the input model mismatches.",
            ));
        }

        let mut padding_buf = vec![0; PADDING_LEN];
        rdr.read_exact(&mut padding_buf)?;

        let mut buffer = Vec::new();
        rdr.read_to_end(&mut buffer)?;

        let mut aligned_bytes = AlignedVec::with_capacity(buffer.len());
        aligned_bytes.extend_from_slice(&buffer);

        let archived = access::<ArchivedDictionaryInner, Error>(&aligned_bytes).map_err(|e| {
            VibratoError::invalid_state(
                "rkyv validation failed. The dictionary file may be corrupted or incompatible."
                    .to_string(),
                e.to_string(),
            )
        })?;

        // SAFETY: AlignedVec ensures correct alignment for ArchivedDictionaryInner
        let data: &'static ArchivedDictionaryInner = unsafe { &*(archived as *const _) };

        Ok(
            Self::Archived(
                ArchivedDictionary { _buffer: DictBuffer::Aligned(aligned_bytes), data }
            )
        )
    }

    /// Creates a dictionary from a file path using memory-mapping for fast loading.
    ///
    /// This function maps a dictionary file into memory for zero-copy access, offering
    /// high performance and memory efficiency. The loading behavior can be configured
    /// with the `mode` parameter to balance safety and performance.
    ///
    /// This function also transparently handles legacy (bincode-based) dictionaries
    /// when the `legacy` feature is enabled, loading them into memory.
    ///
    /// | Mode | Validation | Writes Cache | Use Case |
    /// |------|-------------|---------------|-----------|
    /// | `Validate` | Full validation every time | ❌ | Maximum safety |
    /// | `TrustCache` | Skips if proof file exists | ✅ | Fast reloads |
    ///
    ///
    /// ## Caching Mechanism (`LoadMode::TrustCache`)
    ///
    /// To accelerate subsequent loads, this function uses a cache mechanism when `TrustCache`
    /// mode is enabled. It generates a unique hash from the dictionary file's metadata
    /// (e.g., size, modification time) and looks for a corresponding "proof file"
    /// (e.g., `<hash>.sha256`) to prove the dictionary's validity without a full, slow check.
    ///
    /// The search for this proof file is performed in two locations:
    /// 1.  **Local Cache**: In the same directory as the dictionary file itself. This allows
    ///     for portable caches that can be moved along with the dictionary.
    /// 2.  **Global Cache**: In a system-wide, user-specific cache directory
    ///     (e.g., `~/.cache/vibrato-rkyv` on Linux).
    ///
    /// If a valid proof file is found in either location, the dictionary is loaded instantly
    /// without further validation.
    ///
    /// If no proof file is found, the function performs a full validation. If successful,
    /// it **creates a new proof file in the global cache directory** to speed up the next load.
    /// This ensures that even dictionaries in read-only locations can benefit from caching.
    ///
    /// # Arguments
    ///
    /// - `path` - A path to the dictionary file.
    /// - `mode` - A [`LoadMode`] that specifies the validation strategy:
    ///   - `LoadMode::Validate`: Performs a full validation of the dictionary data on
    ///     every load. This is the safest mode and **never writes cache files**.
    ///     Use this for maximum safety or in environments where file writes are prohibited.
    ///   - `LoadMode::TrustCache`: Enables the caching mechanism described above. It attempts
    ///     a fast, unchecked load if a valid proof file is found. If not, it falls back to
    ///     a full validation and then **creates a proof file in the global cache** on success.
    ///     **Warning: This mode trusts file metadata for validation to achieve high performance.
    ///     It is vulnerable to time-of-check to time-of-use (TOCTOU) attacks if the dictionary
    ///     file can be replaced by a malicious actor. Use `LoadMode::Validate` in environments
    ///     where file integrity cannot be guaranteed.**
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be opened or read.
    /// - The file is corrupted, has an invalid format, or has a mismatched magic number.
    /// - The file was created with an incompatible version of vibrato.
    /// - (`legacy` feature disabled) A legacy bincode-based dictionary is provided.
    pub fn from_path<P: AsRef<std::path::Path>>(path: P, mode: LoadMode) -> Result<Self> {
        let path = path.as_ref();
        let mut file = File::open(path).map_err(|e| {
            VibratoError::invalid_argument("path", format!("Failed to open dictionary file: {}", e))
        })?;
        let meta = &file.metadata()?;
        let mut magic = [0u8; MODEL_MAGIC_LEN];
        file.read_exact(&mut magic)?;

        if magic.starts_with(LEGACY_MODEL_MAGIC_PREFIX) {
            #[cfg(not(feature = "legacy"))]
            return Err(VibratoError::invalid_argument(
                "path",
                "This appears to be a legacy bincode-based dictionary file. Please use a dictionary compiled for the rkyv version of vibrato.",
            ));

            #[cfg(feature = "legacy")]
            {
                use std::io::Seek;
                use crate::legacy;

                file.seek(io::SeekFrom::Start(0))?;

                let dict = legacy::Dictionary::read(file)?.data;

                let dict = unsafe {
                    use std::mem::transmute;

                    Arc::new(transmute::<legacy::dictionary::DictionaryInner, DictionaryInner>(dict))
                };

                return Ok(Self::Owned{ dict, _caching_handle: None });
            }
        } else if !magic.starts_with(MODEL_MAGIC) {
            return Err(VibratoError::invalid_argument(
                "path",
                "The magic number of the input model mismatches.",
            ));
        }

        let mmap = unsafe { Mmap::map(&file)? };

        let Some(data_bytes) = &mmap.get(DATA_START..) else {
            return Err(VibratoError::invalid_argument(
                "path",
                "Dictionary file too small or corrupted.",
            ));
        };

        let current_hash = compute_metadata_hash(meta);
        let hash_name = format!("{}.sha256", current_hash);
        let hash_path = path.parent().unwrap().join(".cache").join(&hash_name);

        if mode == LoadMode::TrustCache
            && hash_path.exists() {
                let archived = unsafe { access_unchecked::<ArchivedDictionaryInner>(data_bytes) };
                let data: &'static ArchivedDictionaryInner = unsafe { &*(archived as *const _) };
                return {
                    Ok(
                        Dictionary::Archived(ArchivedDictionary { _buffer: DictBuffer::Mmap(mmap), data })
                    )
                };
            }

        let global_cache_dir = GLOBAL_CACHE_DIR.as_ref().ok_or_else(|| {
            VibratoError::invalid_state("Could not determine system cache directory.", "")
        })?;

        let hash_path = global_cache_dir.join(&hash_name);

        if mode == LoadMode::TrustCache
            && hash_path.exists() {
                let archived = unsafe { access_unchecked::<ArchivedDictionaryInner>(data_bytes) };
                let data: &'static ArchivedDictionaryInner = unsafe { &*(archived as *const _) };
                return {
                    Ok(
                        Dictionary::Archived(ArchivedDictionary { _buffer: DictBuffer::Mmap(mmap), data })
                    )
                };
            }

        match access::<ArchivedDictionaryInner, Error>(data_bytes) {
            Ok(archived) => {
                if mode == LoadMode::TrustCache {
                    create_dir_all(global_cache_dir)?;
                    File::create(hash_path)?;
                }

                let data: &'static ArchivedDictionaryInner = unsafe { &*(archived as *const _) };
                Ok(Self::Archived(
                    ArchivedDictionary {
                        _buffer: DictBuffer::Mmap(mmap),
                        data,
                    }
                ))
            }
            Err(_) => {
                let mut aligned_bytes = AlignedVec::with_capacity(data_bytes.len());
                aligned_bytes.extend_from_slice(data_bytes);

                let archived = access::<ArchivedDictionaryInner, Error>(&aligned_bytes).map_err(|e| {
                    VibratoError::invalid_state(
                        "rkyv validation failed. The dictionary file may be corrupted or incompatible.".to_string(),
                        e.to_string(),
                    )
                })?;

                let data: &'static ArchivedDictionaryInner = unsafe { &*(archived as *const _) };
                Ok(Self::Archived(
                    ArchivedDictionary {
                        _buffer: DictBuffer::Aligned(aligned_bytes),
                        data,
                    }
                ))
            }
        }
    }

    /// Creates a dictionary from a file path using memory-mapping without validation.
    ///
    /// This function is a version of `from_path` that skips data validation for
    /// faster loading. It memory-maps the dictionary file for zero-copy access.
    /// It is intended for situations where the file's integrity has already been
    /// confirmed, for instance, through a checksum.
    ///
    /// # Arguments
    ///
    /// * `path` - A path to the compiled dictionary file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened, is too small, or has an
    /// incorrect magic number. This function does not validate the integrity of the
    /// serialized data itself.
    ///
    /// # Safety
    ///
    /// This function is unsafe because it bypasses `rkyv`'s validation steps and
    /// directly accesses the memory-mapped data. The caller must ensure that the
    /// file's contents are a valid and uncorrupted representation of a dictionary.
    ///
    /// If the file is corrupted or truncated, this function may read invalid data
    /// as if it were valid pointers or offsets. This can lead to out-of-bounds
    /// memory access, panics, or other forms of undefined behavior.
    ///
    /// The magic number check at the start of the file helps prevent loading a
    /// completely different file type but does not guarantee the integrity of the
    /// subsequent data.
    pub unsafe fn from_path_unchecked<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let mut file = File::open(path).map_err(|e| {
            VibratoError::invalid_argument("path", format!("Failed to open dictionary file: {}", e))
        })?;
        let mut magic = [0u8; MODEL_MAGIC_LEN];
        file.read_exact(&mut magic)?;

        if magic.starts_with(LEGACY_MODEL_MAGIC_PREFIX) {
            #[cfg(not(feature = "legacy"))]
            return Err(VibratoError::invalid_argument(
                "path",
                "This appears to be a legacy bincode-based dictionary file. Please use a dictionary compiled for the rkyv version of vibrato.",
            ));

            #[cfg(feature = "legacy")]
            {
                use std::io::Seek;

                use crate::legacy;

                file.seek(io::SeekFrom::Start(0))?;

                let dict = legacy::Dictionary::read(file)?.data;

                let dict = unsafe {
                    use std::mem::transmute;

                    Arc::new(transmute::<legacy::dictionary::DictionaryInner, DictionaryInner>(dict))
                };

                return Ok(Self::Owned{ dict, _caching_handle: None });
            }
        } else if !magic.starts_with(MODEL_MAGIC) {
            return Err(VibratoError::invalid_argument(
                "path",
                "The magic number of the input model mismatches.",
            ));
        }

        let mmap = unsafe { Mmap::map(&file)? };

        let Some(data_bytes) = &mmap.get(DATA_START..) else {
            return Err(VibratoError::invalid_argument(
                "path",
                "Dictionary file too small or corrupted.",
            ));
        };

        let archived = unsafe { access_unchecked::<ArchivedDictionaryInner>(data_bytes) };
        let data: &'static ArchivedDictionaryInner = unsafe { &*(archived as *const _) };
        Ok(
            Self::Archived(
                ArchivedDictionary {
                    _buffer: DictBuffer::Mmap(mmap),
                    data,
                }
            )
        )
    }

    /// Loads a dictionary from a Zstandard-compressed file using a specified caching strategy.
    ///
    /// This function provides a user-friendly interface for the most common caching scenarios.
    /// For more fine-grained control, see [`from_zstd_with_options`].
    ///
    /// # Arguments
    ///
    /// * `path` - A path to the Zstandard-compressed dictionary file.
    /// * `strategy` - The desired caching strategy, defined by the [`CacheStrategy`] enum.
    #[cfg_attr(feature = "legacy", doc = r"
    When the `legacy` feature is enabled, this function returns immediately while caching
    happens in the background, providing a responsive user experience.")]
    ///
    /// # Errors
    ///
    /// Returns an error if the specified `cache_dir` (determined by the `strategy`)
    /// cannot be created or written to, in addition to the errors from
    /// [`from_zstd_with_options`].
    pub fn from_zstd<P: AsRef<std::path::Path>>(path: P, strategy: CacheStrategy) -> Result<Self> {
        let path = path.as_ref();

        let cache_dir = match strategy {
            CacheStrategy::Local => {
                let parent = path.parent().ok_or_else(|| {
                    VibratoError::invalid_argument(
                        "path",
                        "Input path must have a parent directory for the Local cache strategy.",
                    )
                })?;
                let local_cache = parent.join(".cache");
                std::fs::create_dir_all(&local_cache)?;
                local_cache
            }
            CacheStrategy::GlobalCache => {
                let global_cache = GLOBAL_CACHE_DIR.as_ref().ok_or_else(|| {
                    VibratoError::invalid_state("Could not determine system cache directory.", "")
                })?;
                global_cache.to_path_buf()
            }
            CacheStrategy::GlobalData => {
                let local_data = GLOBAL_DATA_DIR.as_ref().ok_or_else(|| {
                    VibratoError::invalid_state("Could not determine local data directory.", "")
                })?;
                local_data.to_path_buf()
            }
        };

        Self::from_zstd_with_options(
            path,
            cache_dir,
            #[cfg(feature = "legacy")]
            false,
        )
    }

    /// Loads a dictionary from a Zstandard-compressed file with configurable caching options.
    ///
    /// This is an advanced version of [`from_zstd`] that allows for fine-grained control
    /// over the caching directory. It is useful in environments with specific directory
    /// structures or restrictive file system permissions.
    ///
    /// ## Caching Mechanism
    ///
    /// To avoid decompressing the file on every run, this function employs a cache mechanism.
    /// It generates a unique hash from the metadata of the input `.zst` file (such as its
    /// size and modification time). This hash is used as the filename for the decompressed
    /// cache.
    ///
    /// On subsequent runs, if a cache file corresponding to the current metadata hash exists,
    /// the decompression step is skipped entirely, enabling near-instantaneous loading.
    /// If the `.zst` file is modified, its metadata hash will change, and a new cache will be
    /// generated automatically.
    ///
    /// # Arguments
    ///
    /// * `path` - A path to the Zstandard-compressed dictionary file.
    /// * `cache_dir` - The directory where the decompressed dictionary cache will be stored.
    #[cfg_attr(feature = "legacy", doc = r" * `wait_for_cache` - (legacy feature only) If `true` and a legacy (bincode) dictionary is
    provided, the function will block until the conversion to the new format and caching are complete.
    If `false`, it returns immediately with a fully functional dictionary, while the caching
    process runs in a background thread.")]
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - The file specified by `path` cannot be opened or read (e.g., I/O errors).
    /// - The file is not a valid Zstandard-compressed archive.
    /// - The decompressed data is not a valid dictionary file (e.g., corrupted data or
    ///   incorrect magic number).
    /// - The cache directory specified by `cache_dir` cannot be created or written to.
    #[cfg_attr(feature = "legacy", doc = r" - (legacy feature only) The background caching thread panics when `wait_for_cache` is `true`.")]
    ///
    /// # Examples
    ///
    /// ### Specifying a custom cache directory
    ///
    /// ```no_run
    /// # use vibrato_rkyv::{Dictionary, errors::Result};
    /// # fn main() -> Result<()> {
    /// let dict = Dictionary::from_zstd_with_options(
    ///     "path/to/system.dic.zst",
    ///     "/tmp/my_app_cache",
    #[cfg_attr(feature = "legacy", doc = r"true, // Wait for background cache generation to complete")]
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn from_zstd_with_options<P, Q>(
        path: P,
        cache_dir: Q,
        #[cfg(feature = "legacy")]
        wait_for_cache: bool,
    ) -> Result<Self>
    where
        P: AsRef<std::path::Path>,
        Q: AsRef<std::path::Path>,
    {
        let zstd_path = path.as_ref();
        let zstd_file = File::open(zstd_path)?;
        let meta = zstd_file.metadata()?;

        let dict_hash = compute_metadata_hash(&meta);
        let decompressed_dir = cache_dir.as_ref().to_path_buf();

        let decompressed_dict_path = decompressed_dir.join(format!("{}.dic", dict_hash));

        if decompressed_dict_path.exists() {
            return Self::from_path(decompressed_dict_path, LoadMode::TrustCache);
        }

        if !decompressed_dir.exists() {
            create_dir_all(&decompressed_dir)?;
        }

        let mut temp_file = tempfile::NamedTempFile::new_in(&decompressed_dir)?;

        {
            let mut decoder = zstd::Decoder::new(zstd_file)?;

            io::copy(&mut decoder, &mut temp_file)?;
            temp_file.as_file().sync_all()?;
        }
        temp_file.seek(SeekFrom::Start(0))?;

        let mut magic = [0; MODEL_MAGIC_LEN];
        temp_file.read_exact(&mut magic)?;

        #[cfg(feature = "legacy")]
        'l: {
            use std::thread;

            use crate::legacy;

            if !magic.starts_with(LEGACY_MODEL_MAGIC_PREFIX) {
                break 'l;
            }

            let dict = legacy::Dictionary::read(
                zstd::Decoder::new(File::open(zstd_path)?)?
            )?.data;

            let dict = unsafe {
                use std::mem::transmute;

                Arc::new(transmute::<legacy::dictionary::DictionaryInner, DictionaryInner>(dict))
            };


            let dict_for_cache = Arc::clone(&dict);
            let handle = thread::spawn(move || -> Result<()> {
                let mut temp_file = tempfile::NamedTempFile::new_in(&decompressed_dir)?;

                dict_for_cache.write(&mut temp_file)?;

                temp_file.persist(&decompressed_dict_path)?;

                let dict_file = File::open(decompressed_dict_path)?;
                let decompressed_dict_hash = compute_metadata_hash(&dict_file.metadata()?);
                let decompressed_dict_hash_path = decompressed_dir.join(format!("{}.sha256", decompressed_dict_hash));

                File::create(decompressed_dict_hash_path)?;

                Ok(())
            });

            let _caching_handle = if wait_for_cache {
                handle.join().map_err(|e| {
                    let panic_msg = if let Some(s) = e.downcast_ref::<&'static str>() {
                        s.to_string()
                    } else if let Some(s) = e.downcast_ref::<String>() {
                        s.clone()
                    } else {
                        "Unknown panic".to_string()
                    };
                    VibratoError::ThreadPanic(panic_msg)
                })??;

                None
            } else {
                Some(std::sync::Arc::new(handle))
            };

            return Ok(Self::Owned { dict, _caching_handle });
        }

        if magic.starts_with(LEGACY_MODEL_MAGIC_PREFIX) {
            return Err(VibratoError::invalid_argument(
                "path",
                "This appears to be a legacy bincode-based dictionary file. Please use a dictionary compiled for the rkyv version of vibrato.",
            ));
        } else if !magic.starts_with(MODEL_MAGIC) {
            return Err(VibratoError::invalid_argument(
                "path",
                "The magic number of the input model mismatches.",
            ));
        }

        temp_file.seek(SeekFrom::Start(0))?;

        let mut data_bytes = Vec::new();
        temp_file.as_file_mut().read_to_end(&mut data_bytes)?;

        let mut aligned_bytes: AlignedVec = AlignedVec::with_capacity(data_bytes.len());
        aligned_bytes.extend_from_slice(&data_bytes);

        let Some(data_bytes) = &aligned_bytes.get(DATA_START..) else {
            return Err(VibratoError::invalid_argument(
                "path",
                "Dictionary file too small or corrupted.",
            ));
        };

        let _ = access::<ArchivedDictionaryInner, Error>(data_bytes).map_err(|e| {
            VibratoError::invalid_state(
                "rkyv validation failed. The dictionary file may be corrupted or incompatible."
                    .to_string(),
                e.to_string(),
            )
        })?;

        temp_file.persist(&decompressed_dict_path)?;

        let decompressed_dict_hash = compute_metadata_hash(&File::open(&decompressed_dict_path)?.metadata()?);
        let decompressed_dict_hash_path = decompressed_dir.join(format!("{}.sha256", decompressed_dict_hash));

        File::create(decompressed_dict_hash_path)?;

        Self::from_path(decompressed_dict_path, LoadMode::TrustCache)
    }

    /// Creates a [`Dictionary`] instance from a reader for a legacy
    /// `bincode`-based dictionary.
    ///
    /// This function is intended for internal tools such as the `compiler` to
    /// convert old dictionary formats. It loads the entire dictionary into memory.
    ///
    /// This function is only available when the `legacy` feature is enabled.
    ///
    /// # Safety
    ///
    /// This function is `unsafe` because it uses [`std::mem::transmute`] to cast
    /// the dictionary structure deserialized with `bincode`.
    /// It is currently safe as this fork maintains an identical memory layout.
    #[cfg(feature = "legacy")]
    pub unsafe fn from_legacy_reader<R: std::io::Read>(reader: R) -> Result<Self> {
        let legacy_dict_inner = crate::legacy::Dictionary::read(reader)?.data;

        let rkyv_dict_inner = unsafe {
            std::mem::transmute::<
                crate::legacy::dictionary::DictionaryInner,
                DictionaryInner,
            >(legacy_dict_inner)
        };

        Ok(Self::Owned { dict: Arc::new(rkyv_dict_inner), _caching_handle: None })
    }

    /// Creates a `Dictionary` instance from a preset, downloading it if not present.
    ///
    /// This is the most convenient way to get started with a pre-compiled dictionary.
    /// The function first checks if the specified preset dictionary already exists in the
    /// given directory. If it exists and its integrity is verified, it is loaded directly.
    /// Otherwise, the dictionary is downloaded from the official repository to the directory,
    /// and then loaded.
    ///
    /// The downloaded dictionary is compressed with Zstandard. This function transparently
    /// handles decompression and caching for fast subsequent loads via memory-mapping.
    ///
    /// This function is only available when the `download` feature is enabled.
    ///
    /// # Arguments
    ///
    /// * `kind` - The preset dictionary to use (e.g., `PresetDictionaryKind::Ipadic`).
    /// * `dir` - The directory where the dictionary will be stored and cached.
    ///   It is recommended to use a persistent location.
    ///
    /// # Errors
    ///
    /// Returns an error if the download fails (e.g., network issues), if the downloaded
    /// file is corrupted (hash mismatch), or if there are file system permission
    /// errors when creating the cache directory.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use std::path::Path;
    /// # use vibrato_rkyv::{Dictionary, Tokenizer, dictionary::PresetDictionaryKind};
    /// # let dir = Path::new("./cache_dir");
    /// // Download and load the IPADIC preset dictionary.
    /// // The first call will download the file, subsequent calls will use the cache.
    /// let dictionary = Dictionary::from_preset_with_download(
    ///     PresetDictionaryKind::Ipadic,
    ///     dir,
    /// ).unwrap();
    ///
    /// let mut tokenizer = Tokenizer::new(dictionary);
    /// ```
    #[cfg(feature = "download")]
    pub fn from_preset_with_download<P: AsRef<std::path::Path>>(kind: PresetDictionaryKind, dir: P) -> Result<Self> {
        let dict_path = fetch::download_dictionary(kind, dir.as_ref())?;

        Self::from_zstd_with_options(
            dict_path,
            dir,
            #[cfg(feature = "legacy")]
            true,
        )
    }

    /// Downloads a preset dictionary file and returns the path to it.
    ///
    /// Once downloaded, the dictionary can be loaded using [`Dictionary::from_zstd`].
    ///
    /// This function is only available when the `download` feature is enabled.
    ///
    /// # Arguments
    ///
    /// * `kind` - The preset dictionary to download (e.g., `PresetDictionaryKind::Ipadic`).
    /// * `dir` - The directory where the dictionary file will be stored.
    ///
    /// # Returns
    ///
    /// A `Result` containing the `PathBuf` to the downloaded
    /// Zstandard-compressed dictionary file.
    ///
    /// # Errors
    ///
    /// Returns an error if the download fails, the file is corrupted, or if there are
    /// file system permission errors.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use std::path::Path;
    /// # use vibrato_rkyv::{Dictionary, dictionary::PresetDictionaryKind, CacheStrategy};
    /// # let dir = Path::new("./cache_dir");
    /// let dict_path = Dictionary::download_dictionary(
    ///     PresetDictionaryKind::UnidicCwj,
    ///     dir,
    /// ).unwrap();
    ///
    /// println!("Dictionary downloaded to: {:?}", dict_path);
    ///
    /// let dictionary = Dictionary::from_zstd(dict_path, CacheStrategy::Local).unwrap();
    /// ```
    #[cfg(feature = "download")]
    pub fn download_dictionary<P: AsRef<std::path::Path>>(kind: PresetDictionaryKind, dir: P) -> Result<std::path::PathBuf> {
        Ok(fetch::download_dictionary(kind, dir)?)
    }

    /// Decompresses a Zstandard-compressed dictionary to a specified path.
    ///
    /// This function reads a `.zst` compressed dictionary, validates its contents,
    /// and writes the decompressed dictionary to the `output_path`.
    ///
    /// This is a lower-level utility useful for application setup, testing,
    /// or custom cache management.
    ///
    /// # Arguments
    ///
    /// * `input_path` - Path to the Zstandard-compressed dictionary file.
    /// * `output_path` - Path where the decompressed dictionary will be saved.
    ///
    /// # Errors
    ///
    /// Returns an error if the input file cannot be read, is not a valid Zstandard
    /// archive, the decompressed data is not a valid dictionary, or the output
    /// path cannot be written to.
    pub fn decompress_zstd<P, Q>(input_path: P, output_path: Q) -> Result<()>
    where
        P: AsRef<std::path::Path>,
        Q: AsRef<std::path::Path>,
    {
        let input_path = input_path.as_ref();
        let output_path = output_path.as_ref();

        let output_dir = output_path.parent().ok_or_else(|| {
            VibratoError::invalid_argument("output_path", "Output path must have a parent directory.")
        })?;
        std::fs::create_dir_all(output_dir)?;

        let zstd_file = File::open(input_path)?;
        let mut temp_file = tempfile::NamedTempFile::new_in(output_dir)?;

        let mut decoder = zstd::Decoder::new(zstd_file)?;
        io::copy(&mut decoder, &mut temp_file)?;

        temp_file.seek(SeekFrom::Start(0))?;
        let mut magic = [0; MODEL_MAGIC_LEN];
        temp_file.read_exact(&mut magic)?;

        if magic.starts_with(LEGACY_MODEL_MAGIC_PREFIX) {
            return Err(VibratoError::invalid_argument(
                "path",
                "This appears to be a legacy bincode-based dictionary file. Please use a dictionary compiled for the rkyv version of vibrato.",
            ));
        } else if !magic.starts_with(MODEL_MAGIC) {
            return Err(VibratoError::invalid_argument(
                "path",
                "The magic number of the input model mismatches.",
            ));
        }

        temp_file.seek(SeekFrom::Start(0))?;
        let mut data_bytes = Vec::new();
        temp_file.as_file_mut().read_to_end(&mut data_bytes)?;

        let mut aligned_bytes: AlignedVec = AlignedVec::with_capacity(data_bytes.len());
        aligned_bytes.extend_from_slice(&data_bytes);

        let Some(data_bytes) = &aligned_bytes.get(DATA_START..) else {
            return Err(VibratoError::invalid_argument(
                "path",
                "Dictionary file too small or corrupted.",
            ));
        };

        let _ = access::<ArchivedDictionaryInner, Error>(data_bytes).map_err(|e| {
            VibratoError::invalid_state(
                "rkyv validation failed. The dictionary file may be corrupted or incompatible."
                    .to_string(),
                e.to_string(),
            )
        })?;

        temp_file.persist(output_path)?;

        Ok(())
    }
}

#[inline(always)]
pub(crate) fn compute_metadata_hash(meta: &Metadata) -> String {
    let mut hasher = Sha256::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        hasher.update(meta.dev().to_le_bytes());
        hasher.update(meta.ino().to_le_bytes());
        hasher.update(meta.size().to_le_bytes());
        hasher.update(meta.mtime().to_le_bytes());
        hasher.update(meta.mtime_nsec().to_le_bytes());
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        hasher.update(meta.file_size().to_le_bytes());
        hasher.update(meta.last_write_time().to_le_bytes());
        hasher.update(meta.creation_time().to_le_bytes());
        hasher.update(meta.file_attributes().to_le_bytes());
    }

    #[cfg(not(any(unix, windows)))]
    {
        use std::time::SystemTime;

        fn update_system_time(
            time: Result<SystemTime, std::io::Error>,
            hasher: &mut Sha256,
        ) {
            match time.and_then(|t| {
                t.duration_since(SystemTime::UNIX_EPOCH)
                    .map_err(|_| std::io::Error::from(std::io::ErrorKind::Other))
            }) {
                Ok(duration) => {
                    hasher.update(duration.as_secs().to_le_bytes());
                    hasher.update(duration.subsec_nanos().to_le_bytes());
                }
                Err(_) => {
                    hasher.update([0u8; 12]);
                }
            }
        }

        let file_type = meta.file_type();
        let type_byte: u8 = if file_type.is_file() { 0x01 }
        else if file_type.is_dir() { 0x02 }
        else if file_type.is_symlink() { 0x03 }
        else { 0x00 };
        hasher.update([type_byte]);

        let readonly_byte: u8 = if meta.permissions().readonly() { 0x01 } else { 0x00 };
        hasher.update([readonly_byte]);

        hasher.update(meta.len().to_le_bytes());

        update_system_time(meta.modified(), &mut hasher);

        update_system_time(meta.created(), &mut hasher);
    }

    hex::encode(hasher.finalize())
}

impl<'a> DictionaryInnerRef<'a> {
    #[inline(always)]
    pub fn connector(&self) -> ConnectorKindRef<'a> {
        match self {
            DictionaryInnerRef::Archived(archived) => ConnectorKindRef::Archived(archived.connector()),
            DictionaryInnerRef::Owned(owned) => ConnectorKindRef::Owned(owned.connector()),
        }
    }

    #[inline(always)]
    pub(crate) fn word_param(&self, word_idx: WordIdx) -> WordParam {
        match self {
            DictionaryInnerRef::Archived(archived_dict) => {
                archived_dict.word_param(word_idx)
            },
            DictionaryInnerRef::Owned(dict) => {
                dict.word_param(word_idx)
            },
        }
    }
}

impl ArchivedDictionaryInner {
    #[inline(always)]
    pub(crate) fn connector(&self) -> &ArchivedConnectorWrapper {
        &self.connector
    }
    #[inline(always)]
    pub(crate) fn system_lexicon(&self) -> &ArchivedLexicon {
        &self.system_lexicon
    }
    #[inline(always)]
    pub(crate) fn user_lexicon(&self) -> &Archived<Option<Lexicon>> {
        &self.user_lexicon
    }
    #[inline(always)]
    pub(crate) fn char_prop(&self) -> &ArchivedCharProperty {
        &self.char_prop
    }
    #[inline(always)]
    pub(crate) fn unk_handler(&self) -> &ArchivedUnkHandler {
        &self.unk_handler
    }
    /// Gets the word parameter.
    #[inline(always)]
    pub(crate) fn word_param(&self, word_idx: WordIdx) -> WordParam {
        match word_idx.lex_type {
            LexType::System => self.system_lexicon().word_param(word_idx),
            LexType::User => self.user_lexicon().as_ref().unwrap().word_param(word_idx),
            LexType::Unknown => self.unk_handler().word_param(word_idx),
        }
    }

    /// Gets the reference to the feature string.
    #[inline(always)]
    pub fn word_feature(&self, word_idx: WordIdx) -> &str {
        match word_idx.lex_type {
            LexType::System => self.system_lexicon().word_feature(word_idx),
            LexType::User => self.user_lexicon().as_ref().unwrap().word_feature(word_idx),
            LexType::Unknown => self.unk_handler().word_feature(word_idx),
        }
    }
}
