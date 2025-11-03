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

use std::fs::{self, File, Metadata};
use std::io::{self, ErrorKind, Read, Write};
use std::ops::Deref;

#[cfg(feature = "download")]
use std::path::Path;
use std::sync::Arc;

use memmap2::Mmap;
use rkyv::{Archived, access_unchecked};
use rkyv::rancor::Error;
use rkyv::util::AlignedVec;
use rkyv::{
    access, api::serialize_using, ser::allocator::Arena, ser::sharing::Share,
    ser::writer::IoWriter, ser::Serializer, util::with_arena, Archive, Deserialize,
    Serialize,
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

/// Magic bytes identifying Vibrato Tokenizer with rkyv v0.6 model file.
pub const MODEL_MAGIC: &[u8] = b"VibratoTokenizerRkyv 0.6\n";

const MODEL_MAGIC_LEN: usize = MODEL_MAGIC.len();
const RKYV_ALIGNMENT: usize = 16;
const PADDING_LEN: usize = (RKYV_ALIGNMENT - (MODEL_MAGIC_LEN % RKYV_ALIGNMENT)) % RKYV_ALIGNMENT;
const DATA_START: usize = MODEL_MAGIC_LEN + PADDING_LEN;

/// Prefix of magic bytes for legacy bincode-based models.
pub const LEGACY_MODEL_MAGIC_PREFIX: &[u8] = b"VibratoTokenizer 0.";

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

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub enum LoadMode {
    /// Perform validation on every load. (Safest)
    Validate,
    /// Skip validation if a pre-computed hash matches. (Fastest for repeated loads)
    TrustCache,
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

impl Deref for ArchivedDictionary {
    type Target = ArchivedDictionaryInner;
    fn deref(&self) -> &Self::Target {
        self.data
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
    /// This function maps the dictionary file into memory and provides zero-copy access to it,
    /// which is fast and memory-efficient. The loading behavior can be configured
    /// with the `mode` parameter to balance safety and performance.
    ///
    /// # Arguments
    ///
    /// - `path` - A path to the compiled dictionary file.
    /// - `mode` - A [`LoadMode`] that specifies the validation strategy:
    ///   - `LoadMode::Validate`: Performs a full validation of the dictionary data on
    ///     every load. This is the safest mode and does **not** write a hash cache file.
    ///     Use this mode if you need to guarantee validation on every single run or if you
    ///     are in an environment where writing cache files is undesirable (e.g., read-only
    ///     filesystems).
    ///   - `LoadMode::TrustCache`: Attempts a faster load by skipping full validation if a
    ///     valid cache file is found. It checks if a `.sha256` file exists and matches the
    ///     current dictionary file. If it matches, the dictionary is loaded without validation.
    ///     If the hash file is missing, mismatched, or the dictionary is invalid, this mode
    ///     falls back to performing a full validation. Upon successful validation in this
    ///     fallback case, it **creates or updates** the `.sha256` hash file to accelerate
    ///     subsequent loads.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened, is corrupted,
    /// or was created with an incompatible version of vibrato.
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

        let hash_path = path.with_added_extension("sha256");
        let current_hash = compute_metadata_hash(meta);

        if mode == LoadMode::TrustCache
            && hash_path.exists()
            && let Ok(cached_hash) = fs::read_to_string(&hash_path)
            && current_hash == cached_hash {
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
                    fs::write(&hash_path, current_hash)?;
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

    /// Loads a dictionary from a Zstandard-compressed file, with automatic caching.
    ///
    /// This is the recommended way to load a dictionary for most use cases.
    ///
    /// On the first run, this function decompresses the dictionary to a `decompressed`
    /// subdirectory next to the compressed file. Subsequent runs will load the
    /// decompressed cache directly using highly efficient memory mapping, achieving
    /// near-instant startup times.
    ///
    /// The cache is automatically invalidated and regenerated if the original
    /// compressed file is modified.
    ///
    /// For more control over caching behavior, see [`from_zstd_with_options`].
    ///
    /// # Arguments
    ///
    /// * `path` - A path to the Zstandard-compressed dictionary file (e.g., `system.dic.zst`).
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - The `path` does not exist or is a directory.
    /// - The file cannot be opened due to permission errors.
    /// - The file is not a valid Zstandard-compressed stream.
    /// - The cache directory cannot be created, and fallback to a global cache also fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use vibrato_rkyv::{Dictionary, errors::Result};
    /// # fn main() -> Result<()> {
    /// let dict = Dictionary::from_zstd("path/to/system.dic.zst")?;
    /// // The dictionary is now ready to use.
    /// # Ok(())
    /// # }
    /// ```
    pub fn from_zstd<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();

        let cache_dir = path
            .parent()
            // `from_zstd_with_options` checks whether path is a directory, so minimal options suffice.
            .ok_or(VibratoError::PathIsDirectory(path.to_path_buf()))?
            .join("decompressed");

        Self::from_zstd_with_options(
            path,
            &cache_dir,
            true,
            #[cfg(feature = "legacy")]
            false,
        )
    }

    /// Loads a dictionary from a Zstandard-compressed file with configurable caching options.
    ///
    /// This is an advanced version of [`from_zstd`] that allows for fine-grained control
    /// over the caching mechanism. It is useful in environments with specific directory
    /// structures or restrictive file system permissions.
    ///
    /// # Arguments
    ///
    /// * `path` - A path to the Zstandard-compressed dictionary file.
    /// * `cache_dir` - The directory where the decompressed dictionary cache will be stored.
    /// * `allow_fallback` - If `true`, the function will attempt to use a global cache
    ///   directory (e.g., `~/.cache/vibrato-rkyv`) as a fallback if creating `cache_dir`
    ///   fails with a permission error.
    #[cfg_attr(feature = "legacy", doc = r" * `await_caching` - (legacy feature only) If `true` and a legacy dictionary is provided,
    the function will block until the conversion to the new format and caching are complete.
    If `false` (the default for `from_zstd`), it returns immediately with the legacy dictionary loaded in memory,
    while the conversion and caching process runs in a background thread. This is useful for tests or cache pre-warming scripts.")]
    ///
    /// # Errors
    ///
    /// This function will return an error under the same conditions as `from_zstd`, but
    /// the caching behavior depends on the `allow_fallback` flag.
    #[cfg_attr(feature = "legacy", doc = r" - (legacy feature only) The background caching thread panics when `await_caching` is `true`.")]
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
    ///     false, // Disable fallback to global cache
    #[cfg_attr(feature = "legacy", doc = r"true, // Wait for background cache generation to complete")]
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    #[inline(always)]
    pub fn from_zstd_with_options<P: AsRef<std::path::Path>, Q: AsRef<std::path::Path>>(
        path: P,
        cache_dir: Q,
        allow_fallback: bool,
        #[cfg(feature = "legacy")]
        wait_for_cache: bool,
    ) -> Result<Self> {
        let zstd_path = path.as_ref();
        let zstd_file = File::open(zstd_path)?;
        let meta = zstd_file.metadata()?;

        let dict_hash = compute_metadata_hash(&meta);
        let decompressed_dir = cache_dir.as_ref().to_path_buf();

        // `let file = File::open(zstd_path)?` is already guarantees that zstd_path is a File.
        let zstd_file_name_os_str = zstd_path.file_name().unwrap();
        let zstd_file_name = zstd_file_name_os_str.to_string_lossy();

        let zstd_hash_path = decompressed_dir.join(format!("{}.sha256", zstd_file_name));
        let decompressed_path = decompressed_dir.join(
            zstd_path.file_stem().unwrap_or(zstd_file_name_os_str)
        );

        if let Ok(compressed_dict_hash) = fs::read_to_string(&zstd_hash_path)
            && compressed_dict_hash.trim() == dict_hash {
                return Self::from_path(decompressed_path, LoadMode::TrustCache);
            }

        #[cfg(feature = "legacy")]
        let transmuted_path = decompressed_path.with_added_extension("rkyv");
        #[cfg(feature = "legacy")]
        let rkyv_hash_path = decompressed_dir.join(format!("{}.rkyv.sha256", zstd_file_name));

        #[cfg(feature = "legacy")]
        if let Ok(compressed_dict_hash) = fs::read_to_string(&rkyv_hash_path)
            && compressed_dict_hash.trim() == dict_hash {
                return Self::from_path(transmuted_path, LoadMode::TrustCache);
            }

        if let Err(e) = fs::create_dir_all(&decompressed_dir) {
            if e.kind() == ErrorKind::PermissionDenied && allow_fallback && let Some(mut decompressed_dir) = dirs::cache_dir() {
                decompressed_dir.push("vibrato-rkyv/decompressed");
                let zstd_hash_path = decompressed_dir.join(format!("{}.sha256", zstd_file_name));
                let decompressed_path = decompressed_dir.join(
                    zstd_path.file_stem().unwrap_or(zstd_file_name_os_str)
                );

                if let Ok(compressed_dict_hash) = fs::read_to_string(&zstd_hash_path)
                    && compressed_dict_hash.trim() == dict_hash {
                        return Self::from_path(decompressed_path, LoadMode::TrustCache);
                    }
            } else {
                Err(e)?;
            }
        }

        let mut temp_file = tempfile::NamedTempFile::new_in(&decompressed_dir)?;

        {
            let mut decoder = zstd::Decoder::new(zstd_file)?;

            io::copy(&mut decoder, &mut temp_file)?;
            temp_file.as_file().sync_all()?;
        }

        let mmap = unsafe { Mmap::map(temp_file.as_file())? };

        rkyv::access::<ArchivedDictionaryInner, Error>(&mmap[DATA_START..]).map_err(|e| {
            VibratoError::invalid_state(
                "Decompressed dictionary is invalid. The original .zst file may be corrupted.",
                e.to_string(),
            )
        })?;


        let _ = fs::remove_file(&decompressed_path);

        let mut _dict_file = temp_file.persist(&decompressed_path)?;


        #[cfg(feature = "legacy")]
        'l: {
            use std::{io::{Seek, SeekFrom}, thread};

            use crate::legacy;

            let mut dict_file = _dict_file;
            dict_file.seek(SeekFrom::Start(0))?;

            let mut magic = [0; MODEL_MAGIC_LEN];
            dict_file.read_exact(&mut magic)?;

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

                temp_file.persist(&transmuted_path)?;

                fs::write(&rkyv_hash_path, dict_hash)?;

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

        fs::write(&zstd_hash_path, dict_hash)?;

        unsafe { Self::from_path_unchecked(decompressed_path) }
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
    pub fn from_preset_with_download<P: AsRef<Path>>(kind: PresetDictionaryKind, dir: P) -> Result<Self> {
        let dict_path = fetch::download_dictionary(kind, dir)?;

        Self::from_zstd(dict_path)
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
    /// # use vibrato_rkyv::{Dictionary, dictionary::PresetDictionaryKind};
    /// # let dir = Path::new("./cache_dir");
    /// let dict_path = Dictionary::download_dictionary(
    ///     PresetDictionaryKind::Unidic,
    ///     dir,
    /// ).unwrap();
    ///
    /// println!("Dictionary downloaded to: {:?}", dict_path);
    ///
    /// let dictionary = Dictionary::from_zstd(dict_path).unwrap();
    /// ```
    #[cfg(feature = "download")]
    pub fn download_dictionary<P: AsRef<Path>>(kind: PresetDictionaryKind, dir: P) -> Result<std::path::PathBuf> {
        Ok(fetch::download_dictionary(kind, dir)?)
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
