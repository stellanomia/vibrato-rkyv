//! Dictionary for tokenization.
pub(crate) mod character;
pub(crate) mod connector;
pub(crate) mod lexicon;
pub(crate) mod mapper;
pub(crate) mod unknown;

use std::io::Read;

use bincode::{Decode, Encode};

use crate::legacy::common;
use crate::legacy::dictionary::character::CharProperty;
use crate::legacy::dictionary::connector::ConnectorWrapper;
use crate::legacy::dictionary::lexicon::Lexicon;
use crate::legacy::dictionary::mapper::ConnIdMapper;
use crate::legacy::dictionary::unknown::UnkHandler;
use crate::legacy::errors::{Result, VibratoError};


const MODEL_MAGIC: &[u8] = b"VibratoTokenizer 0.5\n";

/// Type of a lexicon that contains the word.
#[derive(Clone, Copy, Eq, PartialEq, Debug, Hash, Decode, Encode)]
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

/// Inner data of [`Dictionary`].
#[derive(Decode, Encode)]
pub struct DictionaryInner {
    pub system_lexicon: Lexicon,
    pub user_lexicon: Option<Lexicon>,
    pub connector: ConnectorWrapper,
    pub mapper: Option<ConnIdMapper>,
    pub char_prop: CharProperty,
    pub unk_handler: UnkHandler,
}

/// Dictionary for tokenization.
pub struct Dictionary {
    pub data: DictionaryInner,
}

impl Dictionary {
    /// Gets the reference to the mapper for connection ids.
    #[allow(dead_code)]
    #[inline(always)]
    pub(crate) const fn mapper(&self) -> Option<&ConnIdMapper> {
        self.data.mapper.as_ref()
    }

    /// Creates a dictionary from raw dictionary data.
    ///
    /// The argument must be a byte sequence exported by the [`Dictionary::write()`] function.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use std::fs::File;
    ///
    /// use vibrato::Dictionary;
    ///
    /// let reader = File::open("path/to/system.dic")?;
    /// let dict = Dictionary::read(reader)?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// When bincode generates an error, it will be returned as is.
    pub fn read<R>(rdr: R) -> Result<Self>
    where
        R: Read,
    {
        Ok(Self {
            data: Self::read_common(rdr)?,
        })
    }

    fn read_common<R>(mut rdr: R) -> Result<DictionaryInner>
    where
        R: Read,
    {
        let mut magic = [0; MODEL_MAGIC.len()];
        rdr.read_exact(&mut magic)?;
        if magic != MODEL_MAGIC {
            return Err(VibratoError::invalid_argument(
                "rdr",
                "The magic number of the input model mismatches.",
            ));
        }
        let config = common::bincode_config();
        let data = bincode::decode_from_std_read(&mut rdr, config)?;
        Ok(data)
    }
}
