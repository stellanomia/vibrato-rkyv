//! Dictionary for tokenization.
pub mod builder;
pub(crate) mod character;
pub(crate) mod connector;
pub(crate) mod lexicon;
pub(crate) mod mapper;
pub(crate) mod unknown;
pub(crate) mod word_idx;

use std::fs::File;
use std::io::{Read, Write};
use std::ops::Deref;

use memmap2::Mmap;
use rkyv::Archived;
use rkyv::rancor::Error;
use rkyv::{
    access, api::serialize_using, ser::allocator::Arena, ser::sharing::Share,
    ser::writer::IoWriter, ser::Serializer, util::with_arena, Archive, Deserialize,
    Serialize,
};

use crate::dictionary::character::{ArchivedCharProperty, CharProperty};
use crate::dictionary::connector::{ArchivedConnectorWrapper, Connector, ConnectorWrapper};
use crate::dictionary::lexicon::{ArchivedLexicon, Lexicon};
use crate::dictionary::mapper::ConnIdMapper;
use crate::dictionary::unknown::{ArchivedUnkHandler, UnkHandler};
use crate::errors::{Result, VibratoError};

pub use crate::dictionary::builder::SystemDictionaryBuilder;
pub use crate::dictionary::word_idx::WordIdx;

pub(crate) use crate::dictionary::lexicon::WordParam;

/// Magic bytes identifying Vibrato Tokenizer v0.6 model file.
pub const MODEL_MAGIC: &[u8] = b"VibratoTokenizer 0.6\n";

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
    Heap(Box<[u8]>),
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
        let file = File::open(path.as_ref()).map_err(|e| {
            VibratoError::invalid_argument(
                "path",
                format!("Failed to open dictionary file: {}", e),
            )
        })?;
        // SAFETY: The file is valid and opened for reading.
        let mmap = unsafe { Mmap::map(&file)? };

        if !mmap.starts_with(MODEL_MAGIC) {
            return Err(VibratoError::invalid_argument(
                "path",
                "The magic number of the input model mismatches.",
            ));
        }

        let data_bytes = &mmap[MODEL_MAGIC.len()..];

        let archived = access::<ArchivedDictionaryInner, Error>(data_bytes).map_err(|e| {
            VibratoError::invalid_state(
                "rkyv validation failed. The dictionary file may be corrupted or incompatible."
                    .to_string(),
                e.to_string(),
            )
        })?;

        // SAFETY: The lifetime of the reference is extended to `'static`.
        // This is safe because the backing `Mmap` object is owned by the struct
        // (in the `_buffer` field), ensuring the memory remains valid as long as
        // the `Dictionary` instance exists.
        let data: &'static ArchivedDictionaryInner = unsafe { &*(archived as *const _) };

        Ok(Self {
            _buffer: DictBuffer::Mmap(mmap),
            data,
        })
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
        let mut magic = [0; MODEL_MAGIC.len()];
        rdr.read_exact(&mut magic)?;
        if magic != MODEL_MAGIC {
            return Err(VibratoError::invalid_argument("rdr", "Magic number mismatch."));
        }

        let mut bytes = Vec::new();
        rdr.read_to_end(&mut bytes)?;
        let boxed_slice = bytes.into_boxed_slice();

        let archived = access::<ArchivedDictionaryInner, Error>(&boxed_slice).map_err(
            |e| {
                VibratoError::invalid_state(
                    "rkyv validation failed. The dictionary file may be corrupted or incompatible."
                        .to_string(),
                    e.to_string(),
                )
            },
        )?;

        // SAFETY: This is safe because the `Box<[u8]>` is moved into this struct,
        // guaranteeing that the memory it points to will remain valid and stable
        // for the lifetime of the `Dictionary` instance.
        let data: &'static ArchivedDictionaryInner = unsafe { &*(archived as *const _) };

        Ok(Self {
            _buffer: DictBuffer::Heap(boxed_slice),
            data,
        })
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
