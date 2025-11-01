pub mod posting;
pub mod trie;

use bincode::{Decode, Encode};

use crate::legacy::dictionary::lexicon::map::posting::Postings;
use crate::legacy::dictionary::lexicon::map::trie::Trie;

#[derive(Decode, Encode)]
pub struct WordMap {
    trie: Trie,
    postings: Postings,
}
