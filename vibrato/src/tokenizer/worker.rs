//! Provider of a routine for tokenization.
use crate::dictionary::{ConnectorKindRef, DictionaryInnerRef};
use crate::dictionary::connector::ConnectorView;
use crate::dictionary::mapper::{ConnIdCounter, ConnIdProbs};
use crate::sentence::Sentence;
use crate::token::{NbestTokenIter, Token, TokenIter};
use crate::tokenizer::lattice::{Lattice, LatticeKind, Node};
use crate::tokenizer::Tokenizer;
use crate::tokenizer::nbest_generator::NbestGenerator;

/// Provider of a routine for tokenization.
///
/// It holds the internal data structures used in tokenization,
/// which can be reused to avoid unnecessary memory reallocation.
pub struct Worker {
    pub(crate) tokenizer: Tokenizer,
    pub(crate) sent: Sentence,
    pub(crate) lattice: LatticeKind,
    pub(crate) top_nodes: Vec<(usize, Node)>,
    pub(crate) counter: Option<ConnIdCounter>,
    pub(crate) nbest_paths: Vec<(Vec<*const Node>, i32)>,
}

impl Worker {
    /// Creates a new instance.
    pub(crate) fn new(tokenizer: Tokenizer) -> Self {
        Self {
            tokenizer,
            sent: Sentence::new(),
            lattice: LatticeKind::For1Best(Lattice::default()),
            top_nodes: vec![],
            counter: None,
            nbest_paths: Vec::with_capacity(0),
        }
    }

    /// Resets the input sentence to be tokenized.
    pub fn reset_sentence<S>(&mut self, input: S)
    where
        S: AsRef<str>,
    {
        self.sent.clear();
        self.top_nodes.clear();
        let input = input.as_ref();
        if !input.is_empty() {
            self.sent.set_sentence(input);
            match self.tokenizer.dictionary() {
                DictionaryInnerRef::Archived(dict) => {
                    self.sent.compile_archived(dict.char_prop());
                },
                DictionaryInnerRef::Owned(dict) => {
                    self.sent.compile(dict.char_prop());
                },
            }
        }
    }

    /// Tokenizes the input sentence set in `state`,
    /// returning the result through `state`.
    pub fn tokenize(&mut self) {
        if self.sent.chars().is_empty() {
            return;
        }
        let lattice_1best = self.lattice.prepare_for_1best(self.sent.len_char());

        self.tokenizer.build_lattice(&self.sent, lattice_1best);
        lattice_1best.append_top_nodes(&mut self.top_nodes);
    }

    /// Tokenizes the sentence and stores the top N-best results internally.
    ///
    /// After calling this, the results can be accessed via `num_nbest_paths()`,
    /// `path_cost(path_idx)`, and `nbest_token_iter(path_idx)`.
    pub fn tokenize_nbest(&mut self, n: usize) {
        self.nbest_paths.clear();
        if self.sent.chars().is_empty() {
            return;
        }
        let lattice_nbest = self.lattice.prepare_for_nbest(self.sent.len_char());

        self.tokenizer.build_lattice_nbest(&self.sent, lattice_nbest);

        let dict_ref = self.tokenizer.dictionary();
        let connector_ref = dict_ref.connector();

        let generator = match connector_ref {
            ConnectorKindRef::Archived(connector) => NbestGenerator::new(lattice_nbest, connector, dict_ref),
            ConnectorKindRef::Owned(connector) => NbestGenerator::new(lattice_nbest, connector, dict_ref),
        };
        self.nbest_paths = generator.take(n).collect();
    }

    /// Gets the number of resultant tokens.
    #[inline(always)]
    pub fn num_tokens(&self) -> usize {
        self.top_nodes.len()
    }

    /// Gets the `i`-th resultant token.
    #[inline(always)]
    pub fn token<'w>(&'w self, i: usize) -> Token<'w> {
        let index = self.num_tokens() - i - 1;
        Token::new(self, index)
    }

    /// Creates an iterator of resultant tokens.
    #[inline(always)]
    pub fn token_iter<'w>(&'w self) -> TokenIter<'w> {
        TokenIter::new(self)
    }

    /// Returns an iterator over the tokens in the N-best path at `path_idx`.
    pub fn nbest_token_iter(&self, path_idx: usize) -> Option<NbestTokenIter<'_>> {
        if path_idx < self.nbest_paths.len() {
            Some(NbestTokenIter::new(self, path_idx))
        } else {
            None
        }
    }

    /// Initializes a counter to compute occurrence probabilities of connection ids.
    pub fn init_connid_counter(&mut self) {
        let (num_left, num_right) = match self.tokenizer.dictionary() {
            DictionaryInnerRef::Archived(dict) =>
                (dict.connector().num_left(), dict.connector().num_right()),
            DictionaryInnerRef::Owned(dict) =>
                (dict.connector().num_left(), dict.connector().num_right()),
        };
        self.counter = Some(ConnIdCounter::new(
            num_left,
            num_right,
        ));
    }

    /// Updates frequencies of connection ids at the last tokenization.
    ///
    /// # Panics
    ///
    /// It will panic when [`Self::init_connid_counter()`] has never been called.
    pub fn update_connid_counts(&mut self) {
        match &self.lattice {
            LatticeKind::For1Best(lattice) => lattice.add_connid_counts(self.counter.as_mut().unwrap()),
            LatticeKind::ForNBest(lattice_nbest) => lattice_nbest.add_connid_counts(self.counter.as_mut().unwrap()),
        }
    }

    /// Computes the computed occurrence probabilities of connection ids,
    /// returning those for left- and right-ids.
    ///
    /// # Panics
    ///
    /// It will panic when [`Self::init_connid_counter()`] has never been called.
    pub fn compute_connid_probs(&self) -> (ConnIdProbs, ConnIdProbs) {
        self.counter.as_ref().unwrap().compute_probs()
    }

    /// Returns the number of N-best paths found.
    pub fn num_nbest_paths(&self) -> usize {
        self.nbest_paths.len()
    }

    /// Returns the total cost of the path at `path_idx`.
    pub fn path_cost(&self, path_idx: usize) -> Option<i32> {
        self.nbest_paths.get(path_idx).map(|(_, cost)| *cost)
    }
}
