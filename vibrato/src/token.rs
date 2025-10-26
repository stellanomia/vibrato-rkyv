//! Container of resultant tokens.
use std::ops::Range;

use crate::dictionary::DictionaryInnerRef;
use crate::dictionary::{word_idx::WordIdx, LexType};
use crate::tokenizer::worker::Worker;

/// Resultant token.
pub struct Token<'w> {
    worker: &'w Worker,
    index: usize,
}

impl<'w> Token<'w> {
    #[inline(always)]
    pub(crate) const fn new(worker: &'w Worker, index: usize) -> Self {
        Self { worker, index }
    }

    /// Gets the position range of the token in characters.
    #[inline(always)]
    pub fn range_char(&self) -> Range<usize> {
        let (end_word, node) = &self.worker.top_nodes[self.index];
        node.start_word..*end_word
    }

    /// Gets the position range of the token in bytes.
    #[inline(always)]
    pub fn range_byte(&self) -> Range<usize> {
        let sent = &self.worker.sent;
        let (end_word, node) = &self.worker.top_nodes[self.index];
        sent.byte_position(node.start_word)..sent.byte_position(*end_word)
    }

    /// Gets the surface string of the token.
    #[inline(always)]
    pub fn surface(&self) -> &'w str {
        let sent = &self.worker.sent;
        &sent.raw()[self.range_byte()]
    }
    /// Gets the word index of the token.
    #[inline(always)]
    pub fn word_idx(&self) -> WordIdx {
        let (_, node) = &self.worker.top_nodes[self.index];
        node.word_idx()
    }

    /// Gets the feature string of the token.
    #[inline(always)]
    pub fn feature(&self) -> &str {
        match self.worker.tokenizer.dictionary() {
            DictionaryInnerRef::Archived(dict) => dict
                .word_feature(self.word_idx()),
            DictionaryInnerRef::Owned(dict) => dict
                .word_feature(self.word_idx()),
        }
    }

    /// Gets the lexicon type where the token is from.
    #[inline(always)]
    pub fn lex_type(&self) -> LexType {
        self.word_idx().lex_type
    }

    /// Gets the left id of the token's node.
    #[inline(always)]
    pub fn left_id(&self) -> u16 {
        let (_, node) = &self.worker.top_nodes[self.index];
        node.left_id
    }

    /// Gets the right id of the token's node.
    #[inline(always)]
    pub fn right_id(&self) -> u16 {
        let (_, node) = &self.worker.top_nodes[self.index];
        node.right_id
    }

    /// Gets the word cost of the token's node.
    #[inline(always)]
    pub fn word_cost(&self) -> i16 {
        let (_, node) = &self.worker.top_nodes[self.index];
        match self.worker.tokenizer.dictionary() {
            DictionaryInnerRef::Archived(dict) => dict
                .word_param(node.word_idx()).word_cost,
            DictionaryInnerRef::Owned(dict) => dict
                .word_param(node.word_idx()).word_cost,
        }
    }

    /// Gets the total cost from BOS to the token's node.
    #[inline(always)]
    pub fn total_cost(&self) -> i32 {
        let (_, node) = &self.worker.top_nodes[self.index];
        node.min_cost
    }

    pub fn to_buf(&self) -> TokenBuf {
        TokenBuf {
            surface: self.surface().to_string(),
            feature: self.feature().to_string(),
            range_char: self.range_char(),
            range_byte: self.range_byte(),
            lex_type: self.lex_type(),
            left_id: self.left_id(),
            right_id: self.right_id(),
            word_cost: self.word_cost(),
            total_cost: self.total_cost(),
        }
    }
}

impl std::fmt::Debug for Token<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Token")
            .field("surface", &self.surface())
            .field("range_char", &self.range_char())
            .field("range_byte", &self.range_byte())
            .field("feature", &self.feature())
            .field("lex_type", &self.lex_type())
            .field("left_id", &self.left_id())
            .field("right_id", &self.right_id())
            .field("word_cost", &self.word_cost())
            .field("total_cost", &self.total_cost())
            .finish()
    }
}

/// Iterator of tokens.
pub struct TokenIter<'w> {
    worker: &'w Worker,
    i: usize,
}

impl<'w> TokenIter<'w> {
    #[inline(always)]
    pub(crate) const fn new(worker: &'w Worker, i: usize) -> Self {
        Self { worker, i }
    }
}

impl<'w> Iterator for TokenIter<'w> {
    type Item = Token<'w>;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        if self.i < self.worker.num_tokens() {
            let t = self.worker.token(self.i);
            self.i += 1;
            Some(t)
        } else {
            None
        }
    }
}

/// An owned, self-contained token.
///
/// This struct is the owned counterpart to [`Token`].
/// It is useful for storing tokenization results or
/// sending them across threads.
#[derive(Debug, Clone)]
pub struct TokenBuf {
    pub surface: String,
    pub feature: String,
    pub range_char: Range<usize>,
    pub range_byte: Range<usize>,
    pub lex_type: LexType,
    pub left_id: u16,
    pub right_id: u16,
    pub word_cost: i16,
    pub total_cost: i32,
}

impl<'w> From<Token<'w>> for TokenBuf {
    fn from(token: Token<'w>) -> Self {
        token.to_buf()
    }
}

#[cfg(test)]
mod tests {
    use crate::dictionary::*;
    use crate::tokenizer::*;

    #[test]
    fn test_iter() {
        let lexicon_csv = "自然,0,0,1,sizen
言語,0,0,4,gengo
処理,0,0,3,shori
自然言語,0,0,6,sizengengo
言語処理,0,0,5,gengoshori";
        let matrix_def = "1 1\n0 0 0";
        let char_def = "DEFAULT 0 1 0";
        let unk_def = "DEFAULT,0,0,100,*";

        let dict_inner =
            SystemDictionaryBuilder::from_readers(
                lexicon_csv.as_bytes(),
                matrix_def.as_bytes(),
                char_def.as_bytes(),
                unk_def.as_bytes(),
            ).unwrap();

        let mut buffer = Vec::new();
        dict_inner.write(&mut buffer).unwrap();

        let dict = Dictionary::read(buffer.as_slice()).unwrap();

        let tokenizer = Tokenizer::new(dict);
        let mut worker = tokenizer.new_worker();
        worker.reset_sentence("自然言語処理");
        worker.tokenize();
        assert_eq!(worker.num_tokens(), 2);

        let mut it = worker.token_iter();
        for i in 0..worker.num_tokens() {
            let lhs = worker.token(i);
            let rhs = it.next().unwrap();
            assert_eq!(lhs.surface(), rhs.surface());
        }
        assert!(it.next().is_none());
    }
}
