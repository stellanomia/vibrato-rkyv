
use bincode::{Decode, Encode};

#[derive(Default, Debug, Clone, Decode, Encode, PartialEq, Eq)]
pub struct UnkEntry {
    pub cate_id: u16,
    pub left_id: u16,
    pub right_id: u16,
    pub word_cost: i16,
    pub feature: String,
}

/// Handler of unknown words.
#[derive(Decode, Encode)]
pub struct UnkHandler {
    offsets: Vec<usize>, // indexed by category id
    entries: Vec<UnkEntry>,
}
