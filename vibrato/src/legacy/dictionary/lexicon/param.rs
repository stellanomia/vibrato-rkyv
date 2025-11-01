use bincode::{Decode, Encode};


#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Decode, Encode)]
pub struct WordParam {
    pub left_id: u16,
    pub right_id: u16,
    pub word_cost: i16,
}

#[derive(Decode, Encode)]
pub struct WordParams {
    params: Vec<WordParam>,
}
