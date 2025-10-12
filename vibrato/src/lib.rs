//! # Vibrato
//!
//! Vibrato is a fast implementation of tokenization (or morphological analysis)
//! based on the viterbi algorithm.
//!
//! ## Examples
//!
//! ```
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//!
//! use vibrato_rkyv::{Dictionary, SystemDictionaryBuilder, Tokenizer};
//!
//! let lexicon_csv = "京都,4,4,5,京都,名詞,固有名詞,地名,一般,*,*,キョウト,京都,*,A,*,*,*,1/5
//! 東京都,5,5,9,東京都,名詞,固有名詞,地名,一般,*,*,トウキョウト,東京都,*,B,5/9,*,5/9,*";
//! let matrix_def = "10 10\n0 4 -5\n0 5 -9";
//! let char_def = "DEFAULT 0 1 0";
//! let unk_def = "DEFAULT,0,0,100,DEFAULT,名詞,普通名詞,*,*,*,*,*,*,*,*,*,*,*,*";
//!
//!
//! let dict_inner = SystemDictionaryBuilder::from_readers(
//!     lexicon_csv.as_bytes(),
//!     matrix_def.as_bytes(),
//!     char_def.as_bytes(),
//!     unk_def.as_bytes(),
//! )?;
//!
//!
//!
//! let mut buffer = Vec::new();
//! dict_inner.write(&mut buffer)?;
//!
//! let dict = Dictionary::read(buffer.as_slice())?;
//!
//!
//! let tokenizer = Tokenizer::new(dict);
//! let mut worker = tokenizer.new_worker();
//!
//! worker.reset_sentence("京都東京都");
//! worker.tokenize();
//! assert_eq!(worker.num_tokens(), 2);
//!
//! let t0 = worker.token(0);
//! assert_eq!(t0.surface(), "京都");
//! assert_eq!(t0.range_char(), 0..2);
//! assert_eq!(t0.range_byte(), 0..6);
//! assert_eq!(t0.feature(), "京都,名詞,固有名詞,地名,一般,*,*,キョウト,京都,*,A,*,*,*,1/5");
//!
//! let t1 = worker.token(1);
//! assert_eq!(t1.surface(), "東京都");
//! assert_eq!(t1.range_char(), 2..5);
//! assert_eq!(t1.range_byte(), 6..15);
//! assert_eq!(t1.feature(), "東京都,名詞,固有名詞,地名,一般,*,*,トウキョウト,東京都,*,B,5/9,*,5/9,*");
//! # Ok(())
//! # }
//! ```
#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(not(any(target_pointer_width = "32", target_pointer_width = "64")))]
compile_error!("`target_pointer_width` must be 32 or 64");

pub mod common;
pub mod dictionary;
pub mod errors;
mod num;
mod sentence;
pub mod token;
pub mod tokenizer;
mod utils;

#[cfg(feature = "train")]
#[cfg_attr(docsrs, doc(cfg(feature = "train")))]
pub mod mecab;

#[cfg(feature = "train")]
#[cfg_attr(docsrs, doc(cfg(feature = "train")))]
pub mod trainer;

#[cfg(all(test, feature = "train"))]
mod test_utils;
#[cfg(test)]
mod tests;

pub use dictionary::{Dictionary, SystemDictionaryBuilder};
pub use tokenizer::Tokenizer;

/// Version number of this library.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
