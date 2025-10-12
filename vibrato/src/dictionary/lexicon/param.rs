use rkyv::{Archive, Deserialize, Serialize};

use crate::dictionary::mapper::ConnIdMapper;

#[derive(Debug, Default, Clone, Copy, Eq, PartialEq, Archive, Serialize, Deserialize)]
pub struct WordParam {
    pub left_id: u16,
    pub right_id: u16,
    pub word_cost: i16,
}

impl WordParam {
    #[inline(always)]
    pub const fn new(left_id: u16, right_id: u16, word_cost: i16) -> Self {
        Self {
            left_id,
            right_id,
            word_cost,
        }
    }
}

impl ArchivedWordParam {
    pub fn to_native(&self) -> WordParam {
        WordParam {
            left_id: self.left_id.to_native(),
            right_id: self.right_id.to_native(),
            word_cost: self.word_cost.to_native(),
        }
    }
}

#[derive(Archive, Serialize, Deserialize)]
pub struct WordParams {
    params: Vec<WordParam>,
}

impl WordParams {
    pub fn new<I>(params: I) -> Self
    where
        I: IntoIterator<Item = WordParam>,
    {
        Self {
            params: params.into_iter().collect(),
        }
    }

    #[inline(always)]
    pub fn get(&self, word_id: usize) -> WordParam {
        self.params[word_id]
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.params.len()
    }

    pub fn map_connection_ids(&mut self, mapper: &ConnIdMapper) {
        for p in &mut self.params {
            p.left_id = mapper.left(p.left_id);
            p.right_id = mapper.right(p.right_id);
        }
    }
}

impl ArchivedWordParams {
    #[inline(always)]
    pub fn get(&self, word_id: usize) -> WordParam {
        self.params[word_id].to_native()
    }
}