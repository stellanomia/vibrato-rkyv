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

use std::fs::{self, File};
use std::io::{self, ErrorKind, Read, Write};
use std::ops::Deref;
use std::path::Path;

use memmap2::Mmap;
use rkyv::Archived;
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
pub use crate::dictionary::config::PresetDictionaryKind;

pub(crate) use crate::dictionary::lexicon::WordParam;

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
pub enum LexType {
    /// System lexicon.
    System,
    /// User lexicon.
    User,
    /// Unknown words.
    Unknown,
}

impl Default for LexType {
    fn default() -> Self {
        Self::System
    }
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
pub struct Dictionary {
    _buffer: DictBuffer,
    // The 'static lifetime is safe because we hold the buffer in `_buffer`.
    data: &'static ArchivedDictionaryInner,
}

impl Deref for Dictionary {
    type Target = ArchivedDictionaryInner;
    fn deref(&self) -> &Self::Target {
        self.data
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

    /// Exports the dictionary data.
    ///
    /// The data is serialized with rkyv.
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
    /// Creates a dictionary from a file path using memory-mapping for fast loading.
    ///
    /// This function maps the dictionary file into memory and provides zero-copy access to it,
    /// which is extremely fast and memory-efficient.
    ///
    /// # Arguments
    ///
    /// * `path` - A path to the compiled dictionary file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened, is corrupted,
    /// or was created with an incompatible version of vibrato.
    pub fn from_path<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let mut file = File::open(path.as_ref()).map_err(|e| {
            VibratoError::invalid_argument("path", format!("Failed to open dictionary file: {}", e))
        })?;
        let mut magic_buf = [0u8; MODEL_MAGIC_LEN];
        file.read_exact(&mut magic_buf)?;

        if magic_buf.starts_with(LEGACY_MODEL_MAGIC_PREFIX) {
            return Err(VibratoError::invalid_argument(
                "path",
                "This appears to be a legacy bincode-based dictionary file. Please use a dictionary compiled for the rkyv version of vibrato.",
            ));
        } else if !magic_buf.starts_with(MODEL_MAGIC) {
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

        #[cfg(not(debug_assertions))]
        {
            use rkyv::access_unchecked;
            let archived = unsafe { access_unchecked::<ArchivedDictionaryInner>(data_bytes) };
            let data: &'static ArchivedDictionaryInner = unsafe { &*(archived as *const _) };
            return Ok(
                Self {
                    _buffer: DictBuffer::Mmap(mmap),
                    data,
                }
            );

        }

        #[allow(unreachable_code)]
        match access::<ArchivedDictionaryInner, Error>(data_bytes) {
            Ok(archived) => {
                let data: &'static ArchivedDictionaryInner = unsafe { &*(archived as *const _) };
                Ok(Self {
                    _buffer: DictBuffer::Mmap(mmap),
                    data,
                })
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
                Ok(Self {
                    _buffer: DictBuffer::Aligned(aligned_bytes),
                    data,
                })
            }
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
        const RKYV_ALIGNMENT: usize = 16;
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

        let padding_len = (RKYV_ALIGNMENT - (MODEL_MAGIC_LEN % RKYV_ALIGNMENT)) % RKYV_ALIGNMENT;
        if padding_len > 0 {
            let mut padding_buf = vec![0; padding_len];
            rdr.read_exact(&mut padding_buf)?;
        }

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

        Ok(Self {
            _buffer: DictBuffer::Aligned(aligned_bytes),
            data,
        })
    }

    /// Loads a dictionary from a Zstandard-compressed file, with automatic caching.
    ///
    /// On the first run, this function decompresses the dictionary to a file in a
    /// `decompressed` subdirectory next to the compressed file. Subsequent runs
    /// will load the decompressed cache directly using the highly efficient `from_path`,
    /// achieving near-instant startup times.
    ///
    /// The cache is automatically invalidated and regenerated if the original compressed
    /// file is modified.
    pub fn from_zstd<P: AsRef<std::path::Path>>(path: P) -> Result<Self> {
        let zstd_path = path.as_ref();
        let mut hasher = Sha256::new();

        let file = File::open(zstd_path)?;
        let meta = file.metadata()?;
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
        }

        let dict_hash = hex::encode(hasher.finalize());
        drop(file);

        let mut decompressed_dir = zstd_path.parent().unwrap().join("decompressed");
        let zstd_file_name_os_str = zstd_path.file_name().unwrap();
        let zstd_file_name = zstd_file_name_os_str.to_string_lossy();
        let zstd_hash_path = decompressed_dir.join(format!("{}.sha256", zstd_file_name));
        let decompressed_path = decompressed_dir.join(
            zstd_path.file_stem().unwrap_or(zstd_file_name_os_str)
        );

        if zstd_hash_path.exists()
            && let Ok(compressed_dict_hash) = fs::read_to_string(&zstd_hash_path)
            && compressed_dict_hash.trim() == dict_hash {
                return Self::from_path(decompressed_path);
            }

        if let Err(e) = fs::create_dir_all(&decompressed_dir) {
            if e.kind() == ErrorKind::PermissionDenied && let Some(mut path) = dirs::cache_dir() {
                path.push("vibrato-rkyv/decompressed");
                decompressed_dir = path;
                let zstd_hash_path = decompressed_dir.join(format!("{}.sha256", zstd_file_name));
                let decompressed_path = decompressed_dir.join(
                    Path::new(&zstd_file_name.to_string()).with_extension("")
                );

                if zstd_hash_path.exists()
                    && let Ok(compressed_dict_hash) = fs::read_to_string(&zstd_hash_path)
                    && compressed_dict_hash.trim() == dict_hash {
                        return Self::from_path(decompressed_path);
                    }
            } else {
                Err(e)?;
            }
        }

        let temp_path = decompressed_dir.join(format!("{}.tmp.{}", zstd_file_name, std::process::id()));

        {
            let compressed_file = File::open(zstd_path)?;
            let mut temp_file = File::create(&temp_path)?;
            let mut decoder = zstd::Decoder::new(compressed_file)?;

            io::copy(&mut decoder, &mut temp_file)?;
            temp_file.sync_all()?;
        }

        fs::rename(&temp_path, &decompressed_path)?;

        fs::write(&zstd_hash_path, dict_hash)?;

        Self::from_path(decompressed_path)
    }

    #[cfg(feature = "download")]
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
    pub fn from_preset_with_download<P: AsRef<Path>>(kind: PresetDictionaryKind, dir: P) -> Result<Self> {
        let dict_path = fetch::download_dictionary(kind, dir)?;

        Self::from_zstd(dict_path)
    }

    #[cfg(feature = "download")]
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
    pub fn download_dictionary<P: AsRef<Path>>(kind: PresetDictionaryKind, dir: P) -> Result<std::path::PathBuf> {
        Ok(fetch::download_dictionary(kind, dir)?)
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
