#[cfg(target_feature = "avx2")]
use std::arch::x86_64::{self, __m256i};

use bincode::{
    de::Decoder,
    enc::Encoder,
    error::{DecodeError, EncodeError},
    Decode, Encode,
};

use crate::legacy::num::U31;


pub const SIMD_SIZE: usize = 8;
#[cfg(not(target_feature = "avx2"))]
#[derive(Clone, Copy)]
pub struct U31x8([U31; SIMD_SIZE]);
#[cfg(target_feature = "avx2")]
#[derive(Clone, Copy)]
pub struct U31x8(__m256i);

impl Default for U31x8 {
    #[cfg(not(target_feature = "avx2"))]
    fn default() -> Self {
        Self([U31::default(); SIMD_SIZE])
    }

    #[cfg(target_feature = "avx2")]
    fn default() -> Self {
        unsafe { Self(x86_64::_mm256_set1_epi32(0)) }
    }
}

impl<Context> Decode<Context> for U31x8 {
    fn decode<D: Decoder>(decoder: &mut D) -> Result<Self, DecodeError> {
        let data: [U31; 8] = Decode::decode(decoder)?;

        // Safety
        debug_assert_eq!(std::mem::size_of_val(data.as_slice()), 32);
        #[cfg(target_feature = "avx2")]
        let data = unsafe { x86_64::_mm256_loadu_si256(data.as_ptr() as *const __m256i) };

        Ok(Self(data))
    }
}
bincode::impl_borrow_decode!(U31x8);

impl Encode for U31x8 {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        #[cfg(not(target_feature = "avx2"))]
        let data = (
            self.0[0], self.0[1], self.0[2], self.0[3], self.0[4], self.0[5], self.0[6], self.0[7],
        );

        #[cfg(target_feature = "avx2")]
        let data = unsafe {
            (
                x86_64::_mm256_extract_epi32(self.0, 0),
                x86_64::_mm256_extract_epi32(self.0, 1),
                x86_64::_mm256_extract_epi32(self.0, 2),
                x86_64::_mm256_extract_epi32(self.0, 3),
                x86_64::_mm256_extract_epi32(self.0, 4),
                x86_64::_mm256_extract_epi32(self.0, 5),
                x86_64::_mm256_extract_epi32(self.0, 6),
                x86_64::_mm256_extract_epi32(self.0, 7),
            )
        };

        Encode::encode(&data, encoder)?;
        Ok(())
    }
}

pub struct Scorer {
    bases: Vec<u32>,
    checks: Vec<u32>,
    costs: Vec<i32>,

    #[cfg(target_feature = "avx2")]
    bases_len: __m256i,
    #[cfg(target_feature = "avx2")]
    checks_len: __m256i,
}

#[allow(clippy::derivable_impls)]
impl Default for Scorer {
    fn default() -> Self {
        Self {
            bases: vec![],
            checks: vec![],
            costs: vec![],

            #[cfg(target_feature = "avx2")]
            bases_len: unsafe { x86_64::_mm256_set1_epi32(0) },
            #[cfg(target_feature = "avx2")]
            checks_len: unsafe { x86_64::_mm256_set1_epi32(0) },
        }
    }
}

impl<Context> Decode<Context> for Scorer {
    fn decode<D: Decoder>(decoder: &mut D) -> Result<Self, DecodeError> {
        let bases: Vec<u32> = Decode::decode(decoder)?;
        let checks: Vec<u32> = Decode::decode(decoder)?;
        let costs: Vec<i32> = Decode::decode(decoder)?;

        if checks.len() != costs.len() {
            return Err(DecodeError::ArrayLengthMismatch {
                required: checks.len(),
                found: costs.len(),
            });
        }

        #[cfg(target_feature = "avx2")]
        let bases_len = unsafe { x86_64::_mm256_set1_epi32(i32::try_from(bases.len()).unwrap()) };
        #[cfg(target_feature = "avx2")]
        let checks_len = unsafe { x86_64::_mm256_set1_epi32(i32::try_from(checks.len()).unwrap()) };

        Ok(Self {
            bases,
            checks,
            costs,

            #[cfg(target_feature = "avx2")]
            bases_len,
            #[cfg(target_feature = "avx2")]
            checks_len,
        })
    }
}
bincode::impl_borrow_decode!(Scorer);

impl Encode for Scorer {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        Encode::encode(&self.bases, encoder)?;
        Encode::encode(&self.checks, encoder)?;
        Encode::encode(&self.costs, encoder)?;
        Ok(())
    }
}
