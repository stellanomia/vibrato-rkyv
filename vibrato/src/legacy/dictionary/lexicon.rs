mod feature;
mod map;
mod param;


use bincode::{Decode, Encode};

use crate::legacy::dictionary::lexicon::feature::WordFeatures;
use crate::legacy::dictionary::lexicon::map::WordMap;
use crate::legacy::dictionary::lexicon::param::WordParams;
use crate::legacy::dictionary::LexType;


/// Lexicon of words.
#[derive(Decode, Encode)]
pub struct Lexicon {
    map: WordMap,
    params: WordParams,
    features: WordFeatures,
    lex_type: LexType,
}
