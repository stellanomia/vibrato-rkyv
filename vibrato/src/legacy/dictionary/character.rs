use std::fmt;

use bincode::{Decode, Encode};


const CATE_IDSET_BITS: usize = 18;
const CATE_IDSET_MASK: u32 = (1 << CATE_IDSET_BITS) - 1;
const BASE_ID_BITS: usize = 8;
const BASE_ID_MASK: u32 = (1 << BASE_ID_BITS) - 1;

/// Information of a character defined in `char.def`.
///
/// The memory layout is
///   cate_idset = 18 bits
///      base_id =  8 bits
///       invoke =  1 bit
///        group =  1 bit
///       length =  4 bits
#[derive(Default, Clone, Copy, Decode, Encode)]
pub struct CharInfo(u32);

impl fmt::Debug for CharInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CharInfo")
            .field("cate_idset", &self.cate_idset())
            .field("base_id", &self.base_id())
            .field("invoke", &self.invoke())
            .field("group", &self.group())
            .field("length", &self.length())
            .finish()
    }
}

impl CharInfo {
    #[inline(always)]
    pub const fn cate_idset(&self) -> u32 {
        self.0 & CATE_IDSET_MASK
    }

    #[inline(always)]
    pub const fn base_id(&self) -> u32 {
        (self.0 >> CATE_IDSET_BITS) & BASE_ID_MASK
    }

    #[inline(always)]
    pub const fn invoke(&self) -> bool {
        (self.0 >> (CATE_IDSET_BITS + BASE_ID_BITS)) & 1 != 0
    }

    #[inline(always)]
    pub const fn group(&self) -> bool {
        (self.0 >> (CATE_IDSET_BITS + BASE_ID_BITS + 1)) & 1 != 0
    }

    #[inline(always)]
    pub const fn length(&self) -> u16 {
        (self.0 >> (CATE_IDSET_BITS + BASE_ID_BITS + 2)) as u16
    }
}

/// Mapping from characters to their information.
#[derive(Decode, Encode)]
pub struct CharProperty {
    chr2inf: Vec<CharInfo>,
    categories: Vec<String>, // indexed by category id
}
