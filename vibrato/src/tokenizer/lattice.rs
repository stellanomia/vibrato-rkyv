use crate::dictionary::connector::ConnectorCost;
use crate::dictionary::lexicon::WordParam;
use crate::dictionary::mapper::ConnIdCounter;
use crate::dictionary::word_idx::WordIdx;
use crate::dictionary::LexType;

use crate::common::{BOS_EOS_CONNECTION_ID, MAX_SENTENCE_LENGTH};

const MAX_COST: i32 = i32::MAX;
const INVALID_IDX: u16 = u16::MAX;

/// A node in the lattice.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Node {
    pub word_id: u32,
    pub lex_type: LexType,
    pub start_node: usize,
    pub start_word: usize,
    pub left_id: u16,
    pub right_id: u16,
    pub min_idx: u16,
    pub min_cost: i32,
    /// A raw pointer to the head of the linked list of paths connecting from the left.
    pub lpath: *const Path, // null if no path
}

/// Represents a connection between two nodes in the lattice.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Path {
    /// A raw pointer to the left node (closer to BOS).
    pub lnode: *const Node,
    /// The next path originating from the right node (linked list).
    pub lnext: *const Path,
}

impl Default for Node {
    fn default() -> Self {
        Self {
            word_id: 0,
            lex_type: LexType::System,
            start_node: 0,
            start_word: 0,
            left_id: 0,
            right_id: 0,
            min_idx: 0,
            min_cost: i32::MAX,
            lpath: std::ptr::null(),
        }
    }
}

impl Node {
    #[inline(always)] pub fn word_idx(&self) -> WordIdx { WordIdx::new(self.lex_type, self.word_id) }
    #[inline(always)] pub fn is_connected_to_bos(&self) -> bool { self.min_cost != MAX_COST }
    #[inline(always)] pub fn is_bos(&self) -> bool { self.start_node == MAX_SENTENCE_LENGTH }
    #[inline(always)] pub fn is_eos(&self) -> bool { self.right_id == u16::MAX }
}

pub enum LatticeKind {
    For1Best(Lattice),
    ForNBest(LatticeNBest),
}

/// This implementation inspired by sudachi.rs.
#[derive(Default)]
pub struct Lattice {
    ends: Vec<Vec<Node>>,
    eos: Option<Node>,
    len_char: usize, // needed for avoiding to free ends
}

impl LatticeKind {
    #[inline]
    pub fn prepare_for_1best(&mut self, len_char: usize) -> &mut Lattice {
        match self {
            LatticeKind::For1Best(l) => {
                l.reset(len_char);
                l
            }
            LatticeKind::ForNBest(_) => {
                *self = LatticeKind::For1Best(Lattice::default());
                self.prepare_for_1best(len_char)
            }
        }
    }

    #[inline]
    pub fn prepare_for_nbest(&mut self, len_char: usize) -> &mut LatticeNBest {
        match self {
            LatticeKind::ForNBest(l) => {
                l.reset(len_char);
                l
            }
            LatticeKind::For1Best(_) => {
                *self = LatticeKind::ForNBest(LatticeNBest::default());
                self.prepare_for_nbest(len_char)
            }
        }
    }
}

impl Lattice {
    pub fn reset(&mut self, len_char: usize) {
        Self::reset_vec(&mut self.ends, len_char + 1);
        self.len_char = len_char;
        self.eos = None;
        self.insert_bos();
    }

    fn reset_vec<T>(data: &mut Vec<Vec<T>>, new_len: usize) {
        for v in data.iter_mut() {
            v.clear();
        }
        let cur_len = data.len();
        if cur_len <= new_len {
            data.reserve(new_len - cur_len);
            for _ in cur_len..new_len {
                data.push(Vec::with_capacity(16))
            }
        }
    }

    /// Returns the number of characters of the set sentence.
    #[inline(always)]
    pub const fn len_char(&self) -> usize {
        self.len_char
    }

    fn insert_bos(&mut self) {
        self.ends[0].push(Node {
            word_id: u32::MAX,
            lex_type: LexType::default(),
            start_node: MAX_SENTENCE_LENGTH,
            start_word: MAX_SENTENCE_LENGTH,
            left_id: u16::MAX,
            right_id: BOS_EOS_CONNECTION_ID,
            min_idx: INVALID_IDX,
            min_cost: 0,
            lpath: std::ptr::null(),
        });
    }

    pub fn insert_eos<C>(&mut self, start_node: usize, connector: &C)
    where
        C: ConnectorCost,
    {
        let (min_idx, min_cost) =
            self.search_min_node(start_node, BOS_EOS_CONNECTION_ID, connector);
        self.eos = Some(Node {
            word_id: u32::MAX,
            lex_type: LexType::default(),
            start_node,
            start_word: self.len_char(),
            left_id: BOS_EOS_CONNECTION_ID,
            right_id: u16::MAX,
            min_idx,
            min_cost,
            lpath: std::ptr::null(),
        });
    }

    pub fn insert_node<C>(
        &mut self,
        start_node: usize,
        start_word: usize,
        end_word: usize,
        word_idx: WordIdx,
        word_param: WordParam,
        connector: &C,
    ) where
        C: ConnectorCost,
    {
        debug_assert!(start_node <= start_word);
        debug_assert!(start_word < end_word);
        let (min_idx, min_cost) = self.search_min_node(start_node, word_param.left_id, connector);
        self.ends[end_word].push(Node {
            word_id: word_idx.word_id,
            lex_type: word_idx.lex_type,
            start_node,
            start_word,
            left_id: word_param.left_id,
            right_id: word_param.right_id,
            min_idx,
            min_cost: min_cost + i32::from(word_param.word_cost),
            lpath: std::ptr::null(),
        });
    }

    fn search_min_node<C>(&self, start_node: usize, left_id: u16, connector: &C) -> (u16, i32)
    where
        C: ConnectorCost,
    {
        debug_assert!(!self.ends[start_node].is_empty());

        let mut min_idx = INVALID_IDX;
        let mut min_cost = MAX_COST;
        for (i, left_node) in self.ends[start_node].iter().enumerate() {
            debug_assert!(left_node.is_connected_to_bos());
            let conn_cost = connector.cost(left_node.right_id, left_id);
            let new_cost = left_node.min_cost + conn_cost;
            // Depending on the order of tie-breaking, the result can be different from MeCab.
            // Using <= (not <) will produce results identical to MeCab in most case (empirically).
            if new_cost <= min_cost {
                min_idx = i as u16;
                min_cost = new_cost;
            }
        }

        debug_assert_ne!(min_idx, INVALID_IDX);
        (min_idx, min_cost)
    }

    /// Checks if there exist at least one at the word end boundary
    #[inline(always)]
    pub fn has_previous_node(&self, i: usize) -> bool {
        self.ends.get(i).map(|d| !d.is_empty()).unwrap_or(false)
    }

    pub fn append_top_nodes(&self, top_nodes: &mut Vec<(usize, Node)>) {
        let eos = self.eos.as_ref().unwrap();
        let mut end_node = eos.start_node;
        let mut min_idx = eos.min_idx;
        while end_node != 0 {
            let node = &self.ends[end_node][usize::from(min_idx)];
            top_nodes.push((end_node, *node));
            (end_node, min_idx) = (node.start_node, node.min_idx);
        }
    }

    pub fn add_connid_counts(&self, counter: &mut ConnIdCounter) {
        for end_char in 1..=self.len_char() {
            for r_node in &self.ends[end_char] {
                let start_node = r_node.start_node;
                for l_node in &self.ends[start_node] {
                    counter.add(r_node.left_id, l_node.right_id, 1);
                }
            }
        }
        let r_node = self.eos.as_ref().unwrap();
        for l_node in &self.ends[self.len_char()] {
            counter.add(r_node.left_id, l_node.right_id, 1);
        }
    }
}

/// This implementation inspired by sudachi.rs.
#[derive(Default)]
pub struct LatticeNBest {
    arena: bumpalo::Bump,
    ends: Vec<Vec<*mut Node>>,
    eos: *mut Node,
    len_char: usize, // needed for avoiding to free ends
}

impl LatticeNBest {
    pub fn reset(&mut self, len_char: usize) {
        self.arena.reset();

        let new_len = len_char + 1;

        for v in self.ends.iter_mut() {
            v.clear();
        }

        let cur_len = self.ends.len();
        if cur_len < new_len {
            self.ends.reserve(new_len - cur_len);
            for _ in cur_len..new_len {
                self.ends.push(Vec::with_capacity(16));
            }
        }

        self.eos = std::ptr::null_mut();
        self.len_char = len_char;
        self.insert_bos();
    }

    /// Gets the EOS node.
    #[inline(always)]
    pub fn eos_node(&self) -> Option<&Node> {
        unsafe { self.eos.as_ref() }
    }

    /// Returns the number of characters of the set sentence.
    #[inline(always)]
    pub const fn len_char(&self) -> usize {
        self.len_char
    }

    fn insert_bos(&mut self) {
        let bos_node = self.arena.alloc(Node {
            word_id: u32::MAX,
            lex_type: LexType::default(),
            start_node: MAX_SENTENCE_LENGTH,
            start_word: MAX_SENTENCE_LENGTH,
            left_id: u16::MAX,
            right_id: BOS_EOS_CONNECTION_ID,
            min_idx: INVALID_IDX,
            min_cost: 0,
            lpath: std::ptr::null(),
        });
        self.ends[0].push(bos_node);
    }

    pub fn insert_eos<C: ConnectorCost>(&mut self, start_node: usize, connector: &C) {
        let eos_node = self.arena.alloc(Node {
            word_id: u32::MAX,
            lex_type: LexType::default(),
            start_node,
            start_word: self.len_char(),
            left_id: BOS_EOS_CONNECTION_ID,
            right_id: u16::MAX,
            ..Default::default()
        });

        let mut min_cost = MAX_COST;
        eos_node.lpath = std::ptr::null();

        for (i, &lnode_ptr) in self.ends[start_node].iter().enumerate() {
            let lnode = unsafe { &*lnode_ptr };
            let conn_cost = connector.cost(lnode.right_id, BOS_EOS_CONNECTION_ID);
            let new_cost = lnode.min_cost + conn_cost;

            if new_cost <= min_cost {
                min_cost = new_cost;
                eos_node.min_idx = i as u16;
            }
            let new_path = self.arena.alloc(Path { lnode: lnode_ptr, lnext: eos_node.lpath });
            eos_node.lpath = new_path;
        }
        eos_node.min_cost = min_cost;
        self.eos = eos_node;
    }

    pub fn insert_node<C>(
        &mut self,
        start_node_pos: usize,
        start_word: usize,
        end_word: usize,
        word_idx: WordIdx,
        word_param: WordParam,
        connector: &C,
    ) where
        C: ConnectorCost,
    {
        debug_assert!(start_node_pos <= start_word);
        debug_assert!(start_word < end_word);

        let rnode_ptr = self.arena.alloc(Node {
            word_id: word_idx.word_id,
            lex_type: word_idx.lex_type,
            start_node: start_node_pos,
            start_word,
            left_id: word_param.left_id,
            right_id: word_param.right_id,
            ..Default::default()
        });
        let rnode = &mut *rnode_ptr;

        let mut min_cost = MAX_COST;
        let mut min_idx = INVALID_IDX;

        rnode.lpath = std::ptr::null();

        for (i, &lnode_ptr) in self.ends[start_node_pos].iter().enumerate() {
            let lnode = unsafe { &*lnode_ptr };
            if !lnode.is_connected_to_bos() {
                continue;
            }

            let conn_cost = connector.cost(lnode.right_id, rnode.left_id);
            let new_cost = lnode.min_cost.saturating_add(conn_cost);
            // Depending on the order of tie-breaking, the result can be different from MeCab.
            // Using <= (not <) will produce results identical to MeCab in most case (empirically).
            if new_cost <= min_cost {
                min_cost = new_cost;
                min_idx = i as u16;
            }

            let new_path = self.arena.alloc(Path {
                lnode: lnode_ptr,
                lnext: rnode.lpath,
            });
            rnode.lpath = new_path;
        }

        if min_idx != INVALID_IDX {
            rnode.min_idx = min_idx;
            rnode.min_cost = min_cost.saturating_add(i32::from(word_param.word_cost));
            self.ends[end_word].push(rnode_ptr);
        }
    }

    /// Checks if there exist at least one at the word end boundary
    #[inline(always)]
    pub fn has_previous_node(&self, i: usize) -> bool {
        self.ends.get(i).map(|d| !d.is_empty()).unwrap_or(false)
    }

    pub fn add_connid_counts(&self, counter: &mut ConnIdCounter) {
        for end_char in 1..=self.len_char() {
            for &r_node_ptr in &self.ends[end_char] {
                let r_node = unsafe { &*r_node_ptr };
                let start_node = r_node.start_node;

                for &l_node_ptr in &self.ends[start_node] {
                    let l_node = unsafe { &*l_node_ptr };
                    counter.add(r_node.left_id, l_node.right_id, 1);
                }
            }
        }

        if !self.eos.is_null() {
            let r_node = unsafe { &*self.eos };
            if let Some(last_nodes) = self.ends.get(r_node.start_node) {
                for &l_node_ptr in last_nodes {
                    let l_node = unsafe { &*l_node_ptr };
                    counter.add(r_node.left_id, l_node.right_id, 1);
                }
            }
        }
    }
}

impl std::fmt::Debug for Lattice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Lattice {{ eos: {:?}, ends: [", &self.eos)?;
        for (i, e) in self.ends[..=self.len_char()].iter().enumerate() {
            writeln!(f, "{i} => {e:?}")?;
        }
        writeln!(f, "]}}")
    }
}
