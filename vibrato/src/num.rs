use rkyv::{Archive, Deserialize, Serialize};

/// Represents an integer from 0 to 2^31 - 1.
///
/// This type guarantees that the sign bit of a 32-bit integer is always zero.
#[derive(Clone, Copy, Default, Eq, PartialEq, Debug, Hash, PartialOrd, Ord, Archive, Serialize, Deserialize)]
#[rkyv(compare(PartialEq), derive(Clone, Copy))]
#[repr(transparent)]
pub struct U31(pub u32);

impl U31 {
    pub const MAX: Self = Self(0x7fff_ffff);

    #[inline(always)]
    pub const fn new(x: u32) -> Option<Self> {
        if x <= Self::MAX.get() {
            Some(Self(x))
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl ArchivedU31 {
    pub fn to_native(self) -> U31 {
        U31(self.0.to_native())
    }
}
