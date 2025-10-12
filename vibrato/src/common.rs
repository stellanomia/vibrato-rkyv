//! Common settings in Vibrato.

/// The maximam length of an input sentence.
///
/// Note that the value must be represented with u16 so that
/// an (exclusive) end position can be represented in 16 bits.
pub const MAX_SENTENCE_LENGTH: usize = usize::MAX;

/// The fixed connection id of BOS/EOS.
pub const BOS_EOS_CONNECTION_ID: u16 = 0;
