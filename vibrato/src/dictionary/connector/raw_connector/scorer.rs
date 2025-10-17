#![allow(dead_code)]
use std::collections::BTreeMap;
use rkyv::rancor::Error;

#[cfg(target_feature = "avx2")]
use std::arch::x86_64 as x86_64;
#[cfg(target_feature = "avx2")]
use avx2_support::M256i;
#[cfg(target_feature = "avx2")]
use rkyv::with::Skip;

use rkyv::{Archive, Deserialize, Serialize, from_bytes_unchecked, to_bytes};

use crate::num::U31;
use crate::utils::FromU32;

const UNUSED_CHECK: u32 = u32::MAX;

pub const SIMD_SIZE: usize = 8;

#[derive(Clone, Copy, Debug, Archive, Serialize, Deserialize, PartialEq, Eq)]
#[rkyv(compare(PartialEq), derive(Clone, Copy))]
pub struct U31x8(pub [U31; SIMD_SIZE]);

impl U31x8 {
    pub fn to_simd_vec(data: &[U31]) -> Vec<Self> {
        let mut result = vec![];
        for xs in data.chunks(SIMD_SIZE) {
            let mut array = [U31::default(); SIMD_SIZE];
            array[..xs.len()].copy_from_slice(xs);
            result.push(Self(array));
        }
        result
    }

    #[cfg(target_feature = "avx2")]
    pub unsafe fn as_m256i(&self) -> x86_64::__m256i {
        unsafe {
            x86_64::_mm256_loadu_si256(self.0.as_ptr() as *const x86_64::__m256i)
        }
    }
}

impl Default for U31x8 {
    fn default() -> Self {
        Self([U31::default(); SIMD_SIZE])
    }
}

pub struct ScorerBuilder {
    // Two-level trie mapping a pair of two keys into a cost
    pub trie: Vec<BTreeMap<U31, i32>>,
}

impl ScorerBuilder {
    pub const fn new() -> Self {
        Self { trie: vec![] }
    }

    pub fn insert(&mut self, key1: U31, key2: U31, cost: i32) {
        let key1 = usize::from_u32(key1.get());
        if key1 >= self.trie.len() {
            self.trie.resize(key1 + 1, BTreeMap::new());
        }
        self.trie[key1].insert(key2, cost);
    }

    #[inline(always)]
    fn check_base(base: u32, second_map: &BTreeMap<U31, i32>, checks: &[u32]) -> bool {
        for &key2 in second_map.keys() {
            if let Some(check) = checks.get(usize::from_u32(base ^ key2.get()))
                && *check != UNUSED_CHECK {
                    return false;
                }
        }
        true
    }

    pub fn build(&self) -> Scorer {
        let mut bases = vec![0; self.trie.len()];
        let mut checks = vec![];
        let mut costs = vec![];
        for (key1, second_map) in self.trie.iter().enumerate() {
            let mut base = 0;
            while !Self::check_base(base, second_map, &checks) {
                base += 1;
            }
            bases[key1] = base;
            for (key2, cost) in second_map {
                let pos = base ^ key2.get();
                let pos = usize::from_u32(pos);
                if pos >= checks.len() {
                    checks.resize(pos + 1, UNUSED_CHECK);
                    costs.resize(pos + 1, 0);
                }
                checks[pos] = u32::try_from(key1).unwrap();
                costs[pos] = *cost;
            }
        }

        #[cfg(target_feature = "avx2")]
        let bases_len = unsafe { x86_64::_mm256_set1_epi32(i32::try_from(bases.len()).unwrap()) };
        #[cfg(target_feature = "avx2")]
        let checks_len = unsafe { x86_64::_mm256_set1_epi32(i32::try_from(checks.len()).unwrap()) };

        Scorer {
            bases,
            checks,
            costs,

            #[cfg(target_feature = "avx2")]
            bases_len: M256i(bases_len),
            #[cfg(target_feature = "avx2")]
            checks_len: M256i(checks_len),
        }
    }
}

#[cfg(target_feature = "avx2")]
mod avx2_support {
    use std::arch::x86_64 as x86_64;

    #[derive(Debug, Clone, Copy)]
    #[repr(transparent)]
    pub struct M256i(pub x86_64::__m256i);

    impl Default for M256i {
        fn default() -> Self {
            unsafe {
                Self(x86_64::_mm256_setzero_si256())
            }
        }
    }
}

#[derive(Debug, Archive, Serialize, Deserialize)]
pub struct Scorer {
    bases: Vec<u32>,
    checks: Vec<u32>,
    costs: Vec<i32>,

    #[cfg(target_feature = "avx2")]
    #[rkyv(with = Skip)]
    bases_len: M256i,

    #[cfg(target_feature = "avx2")]
    #[rkyv(with = Skip)]
    checks_len: M256i,
}

#[allow(clippy::derivable_impls)]
impl Default for Scorer {
    fn default() -> Self {
        Self {
            bases: vec![],
            checks: vec![],
            costs: vec![],

            #[cfg(target_feature = "avx2")]
            bases_len: M256i(unsafe { x86_64::_mm256_set1_epi32(0) }),
            #[cfg(target_feature = "avx2")]
            checks_len: M256i(unsafe { x86_64::_mm256_set1_epi32(0) }),
        }
    }
}

impl Scorer {
    #[cfg(not(target_feature = "avx2"))]
    #[inline(always)]
    fn retrieve_cost(&self, key1: U31, key2: U31) -> Option<i32> {
        if let Some(base) = self.bases.get(usize::from_u32(key1.get())) {
            let pos = base ^ key2.get();
            let pos = usize::from_u32(pos);
            if let Some(check) = self.checks.get(pos)
                && *check == key1.get() {
                    return Some(self.costs[pos]);
                }
        }
        None
    }

    #[cfg(not(target_feature = "avx2"))]
    #[inline(always)]
    pub fn accumulate_cost(&self, keys1: &[U31x8], keys2: &[U31x8]) -> i32 {
        let mut score = 0;
        for (key1, key2) in keys1.iter().zip(keys2) {
            for (&k1, &k2) in key1.0.iter().zip(&key2.0) {
                if let Some(w) = self.retrieve_cost(k1, k2) {
                    score += w;
                }
            }
        }
        score
    }

    #[cfg(target_feature = "avx2")]
    #[inline(always)]
    pub unsafe fn retrieve_cost(&self, key1: x86_64::__m256i, key2: x86_64::__m256i) -> x86_64::__m256i {
        unsafe {
            // key1 < bases.len() ?
            let mask_valid_key1 = x86_64::_mm256_cmpgt_epi32(self.bases_len.0, key1);
            // base = bases[key1]
            let base = x86_64::_mm256_mask_i32gather_epi32(
                x86_64::_mm256_set1_epi32(0),
                self.bases.as_ptr() as *const i32,
                key1,
                mask_valid_key1,
                4,
            );
            // pos = base ^ key2
            let pos = x86_64::_mm256_xor_si256(base, key2);
            // pos < checks.len() && key1 < bases.len() ?
            let mask_valid_pos = x86_64::_mm256_and_si256(
                x86_64::_mm256_cmpgt_epi32(self.checks_len.0, pos),
                mask_valid_key1,
            );
            // check = checks[pos]
            let check = x86_64::_mm256_mask_i32gather_epi32(
                x86_64::_mm256_set1_epi32(UNUSED_CHECK as i32),
                self.checks.as_ptr() as *const i32,
                pos,
                mask_valid_pos,
                4,
            );
            // check == key1 && pos < checks.len() && key1 < bases.len() ?
            let mask_checked =
                x86_64::_mm256_and_si256(x86_64::_mm256_cmpeq_epi32(check, key1), mask_valid_pos);

            x86_64::_mm256_mask_i32gather_epi32(
                x86_64::_mm256_set1_epi32(0),
                self.costs.as_ptr(),
                pos,
                mask_checked,
                4,
            )
        }
    }

    #[cfg(target_feature = "avx2")]
    #[inline(always)]
    pub fn accumulate_cost(&self, keys1: &[U31x8], keys2: &[U31x8]) -> i32 {
        unsafe {
            let mut sums = x86_64::_mm256_set1_epi32(0);
            for (k1, k2) in keys1.iter().zip(keys2.iter()) {
                let key1 = k1.as_m256i();
                let key2 = k2.as_m256i();

                sums = x86_64::_mm256_add_epi32(sums, self.retrieve_cost(key1, key2));
            }
            x86_64::_mm256_extract_epi32(sums, 0)
                + x86_64::_mm256_extract_epi32(sums, 1)
                + x86_64::_mm256_extract_epi32(sums, 2)
                + x86_64::_mm256_extract_epi32(sums, 3)
                + x86_64::_mm256_extract_epi32(sums, 4)
                + x86_64::_mm256_extract_epi32(sums, 5)
                + x86_64::_mm256_extract_epi32(sums, 6)
                + x86_64::_mm256_extract_epi32(sums, 7)
        }
    }

    pub fn serialize_to_bytes(&self) -> Vec<u8> {
        to_bytes::<Error>(self).expect("failed to rkyv serialize").into()
    }

    pub unsafe fn deserialize_from_bytes(bytes: &[u8]) -> Scorer {
        unsafe { from_bytes_unchecked::<Scorer, Error>(bytes).expect("failed to rkyv deserialize") }
    }
}

impl ArchivedScorer {
    #[cfg(target_feature = "avx2")]
    unsafe fn post_deserialize(&self) -> (x86_64::__m256i, x86_64::__m256i) {
        unsafe {
            let bases_len = x86_64::_mm256_set1_epi32(i32::try_from(self.bases.len()).unwrap());
            let checks_len = x86_64::_mm256_set1_epi32(i32::try_from(self.checks.len()).unwrap());
            (bases_len, checks_len)
        }
    }

    #[cfg(not(target_feature = "avx2"))]
    #[inline(always)]
    fn retrieve_cost(&self, key1: U31, key2: U31) -> Option<i32> {
        if let Some(&base_le) = self.bases.get(usize::from_u32(key1.get())) {
            let base = base_le.to_native();
            let pos = base ^ key2.get();
            let pos = usize::from_u32(pos);
            if let Some(&check_le) = self.checks.get(pos) {
                let check = check_le.to_native();
                if check == key1.get() {
                    return Some(self.costs[pos].to_native());
                }
            }
        }
        None
    }

    #[cfg(not(target_feature = "avx2"))]
    #[inline(always)]
    pub fn accumulate_cost(&self, keys1: &[ArchivedU31x8], keys2: &[ArchivedU31x8]) -> i32 {
        let mut score = 0;
        for (key1, key2) in keys1.iter().zip(keys2) {
            for (k1, k2) in key1.0.iter().zip(&key2.0) {
                if let Some(w) = self.retrieve_cost(k1.to_native(), k2.to_native()) {
                    score += w;
                }
            }
        }
        score
    }

    #[cfg(target_feature = "avx2")]
    #[inline(always)]
    pub unsafe fn retrieve_cost(
        &self,
        key1: x86_64::__m256i,
        key2: x86_64::__m256i,
        bases_len: x86_64::__m256i,
        checks_len: x86_64::__m256i,
    ) -> x86_64::__m256i {
        unsafe {
            // key1 < bases.len() ?
            let mask_valid_key1 = x86_64::_mm256_cmpgt_epi32(bases_len, key1);

            // base = bases[key1]
            let base = x86_64::_mm256_mask_i32gather_epi32(
                x86_64::_mm256_set1_epi32(0),
                self.bases.as_ptr() as *const i32,
                key1,
                mask_valid_key1,
                4, // 4 bytes (i32) scale
            );

            // pos = base ^ key2
            let pos = x86_64::_mm256_xor_si256(base, key2);

            // pos < checks.len() && key1 < bases.len() ?
            let mask_valid_pos = x86_64::_mm256_and_si256(
                x86_64::_mm256_cmpgt_epi32(checks_len, pos),
                mask_valid_key1,
            );

            // check = checks[pos]
            let check = x86_64::_mm256_mask_i32gather_epi32(
                x86_64::_mm256_set1_epi32(UNUSED_CHECK as i32),
                self.checks.as_ptr() as *const i32,
                pos,
                mask_valid_pos,
                4,
            );

            // check == key1 && pos < checks.len() && key1 < bases.len() ?
            let mask_checked =
                x86_64::_mm256_and_si256(x86_64::_mm256_cmpeq_epi32(check, key1), mask_valid_pos);

            // return costs[pos] where mask is set
            x86_64::_mm256_mask_i32gather_epi32(
                x86_64::_mm256_set1_epi32(0),
                self.costs.as_ptr() as *const i32,
                pos,
                mask_checked,
                4,
            )
        }
    }

    #[cfg(target_feature = "avx2")]
    #[inline(always)]
    pub fn accumulate_cost(&self, keys1: &[ArchivedU31x8], keys2: &[ArchivedU31x8]) -> i32 {
        unsafe {
            let (bases_len, checks_len) = self.post_deserialize();

            let mut sums = x86_64::_mm256_set1_epi32(0);
            for (k1, k2) in keys1.iter().zip(keys2.iter()) {
                let key1 = k1.as_m256i();
                let key2 = k2.as_m256i();

                sums = x86_64::_mm256_add_epi32(sums, self.retrieve_cost(key1, key2, bases_len, checks_len));
            }

            // Sum up all 8 lanes of the SIMD register
            x86_64::_mm256_extract_epi32(sums, 0)
                + x86_64::_mm256_extract_epi32(sums, 1)
                + x86_64::_mm256_extract_epi32(sums, 2)
                + x86_64::_mm256_extract_epi32(sums, 3)
                + x86_64::_mm256_extract_epi32(sums, 4)
                + x86_64::_mm256_extract_epi32(sums, 5)
                + x86_64::_mm256_extract_epi32(sums, 6)
                + x86_64::_mm256_extract_epi32(sums, 7)
        }
    }
}

impl ArchivedU31x8 {
    #[cfg(target_feature = "avx2")]
    pub unsafe fn as_m256i(&self) -> x86_64::__m256i {
        unsafe {
            x86_64::_mm256_loadu_si256(self.0.as_ptr() as *const x86_64::__m256i)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rkyv::rancor::Error;
    use crate::dictionary::connector::raw_connector::INVALID_FEATURE_ID;

    fn build_test_scorer() -> Scorer {
        let mut builder = ScorerBuilder::new();
        builder.insert(U31::new(18).unwrap(), U31::new(17).unwrap(), 1);
        builder.insert(U31::new(4).unwrap(), U31::new(9).unwrap(), 2);
        builder.insert(U31::new(17).unwrap(), U31::new(0).unwrap(), 3);
        builder.insert(U31::new(17).unwrap(), U31::new(12).unwrap(), 4);
        builder.insert(U31::new(8).unwrap(), U31::new(6).unwrap(), 5);
        builder.insert(U31::new(2).unwrap(), U31::new(5).unwrap(), 6);
        builder.insert(U31::new(12).unwrap(), U31::new(18).unwrap(), 7);
        builder.insert(U31::new(9).unwrap(), U31::new(1).unwrap(), 8);
        builder.insert(U31::new(19).unwrap(), U31::new(5).unwrap(), 9);
        builder.insert(U31::new(9).unwrap(), U31::new(4).unwrap(), 10);
        builder.insert(U31::new(0).unwrap(), U31::new(19).unwrap(), 11);
        builder.insert(U31::new(2).unwrap(), U31::new(19).unwrap(), 12);
        builder.insert(U31::new(7).unwrap(), U31::new(9).unwrap(), 13);
        builder.insert(U31::new(18).unwrap(), U31::new(9).unwrap(), 14);
        builder.insert(U31::new(17).unwrap(), U31::new(4).unwrap(), 15);
        builder.insert(U31::new(9).unwrap(), U31::new(6).unwrap(), 16);
        builder.insert(U31::new(13).unwrap(), U31::new(0).unwrap(), 17);
        builder.insert(U31::new(1).unwrap(), U31::new(4).unwrap(), 18);
        builder.insert(U31::new(0).unwrap(), U31::new(18).unwrap(), 19);
        builder.insert(U31::new(18).unwrap(), U31::new(11).unwrap(), 20);
        builder.build()
    }

    #[test]
    fn roundtrip_serialize_and_accumulate_cost() {
        let scorer = build_test_scorer();

        let bytes = scorer.serialize_to_bytes();

        #[allow(unused_mut)]
        let mut restored_scorer = rkyv::from_bytes::<Scorer, Error>(&bytes).expect("deserialization failed");

        #[cfg(target_feature = "avx2")]
        {
            restored_scorer.bases_len = M256i(unsafe { x86_64::_mm256_set1_epi32(i32::try_from(restored_scorer.bases.len()).unwrap()) });
            restored_scorer.checks_len = M256i(unsafe { x86_64::_mm256_set1_epi32(i32::try_from(restored_scorer.checks.len()).unwrap()) });
        }

        assert_eq!(restored_scorer.bases, scorer.bases);
        assert_eq!(restored_scorer.checks, scorer.checks);
        assert_eq!(restored_scorer.costs, scorer.costs);

        let keys1 = U31x8::to_simd_vec(&[
            U31::new(18).unwrap(), U31::new(17).unwrap(), U31::new(0).unwrap(), INVALID_FEATURE_ID,
            U31::new(8).unwrap(), U31::new(12).unwrap(), U31::new(19).unwrap(), INVALID_FEATURE_ID,
            INVALID_FEATURE_ID, U31::new(9).unwrap(), U31::new(0).unwrap(), U31::new(7).unwrap(),
            U31::new(17).unwrap(), U31::new(13).unwrap(), U31::new(0).unwrap(), INVALID_FEATURE_ID
        ]);
        let keys2 = U31x8::to_simd_vec(&[
            U31::new(17).unwrap(), U31::new(0).unwrap(), U31::new(0).unwrap(), INVALID_FEATURE_ID,
            U31::new(6).unwrap(), U31::new(18).unwrap(), U31::new(5).unwrap(), INVALID_FEATURE_ID,
            INVALID_FEATURE_ID, U31::new(9).unwrap(), U31::new(19).unwrap(), U31::new(9).unwrap(),
            U31::new(4).unwrap(), U31::new(0).unwrap(), U31::new(18).unwrap(), INVALID_FEATURE_ID
        ]);

        assert_eq!(restored_scorer.accumulate_cost(&keys1, &keys2), 100);
    }

    #[test]
    fn retrieve_cost_test() {
        let scorer = build_test_scorer();

        let cases = vec![
            (0, 18, Some(19)),
            (0, 19, Some(11)),
            (9, 4, Some(10)),
            (9, 6, Some(16)),
            (0, 0, None),
            (9, 5, None),
        ];

        #[cfg(not(target_feature = "avx2"))]
        {
            for (k1, k2, expected) in cases {
                assert_eq!(
                    scorer.retrieve_cost(U31::new(k1).unwrap(), U31::new(k2).unwrap()),
                    expected
                );
            }
        }

        #[cfg(target_feature = "avx2")]
        unsafe {
            let mut k1_vec = [0i32; 8];
            let mut k2_vec = [0i32; 8];
            let mut expected_vec = [0i32; 8];

            for (i, (k1, k2, expected)) in cases.iter().enumerate() {
                k1_vec[i] = *k1 as i32;
                k2_vec[i] = *k2 as i32;
                expected_vec[i] = expected.unwrap_or(0);
            }

            let k1_simd = x86_64::_mm256_loadu_si256(k1_vec.as_ptr() as *const _);
            let k2_simd = x86_64::_mm256_loadu_si256(k2_vec.as_ptr() as *const _);

            let result_simd = scorer.retrieve_cost(k1_simd, k2_simd);

            let mut result_vec = [0i32; 8];
            x86_64::_mm256_storeu_si256(result_vec.as_mut_ptr() as *mut _, result_simd);

            assert_eq!(result_vec, expected_vec);
        }
    }

    #[test]
    fn u31x8_serialize_roundtrip() {
        let data = U31x8([
            U31::new(0).unwrap(), U31::new(1).unwrap(), U31::new(2).unwrap(), U31::new(3).unwrap(),
            U31::new(4).unwrap(), U31::new(5).unwrap(), U31::new(6).unwrap(), U31::new(7).unwrap(),
        ]);

        let bytes = rkyv::to_bytes::<Error>(&data).unwrap();
        let decoded = rkyv::from_bytes::<U31x8, Error>(&bytes).unwrap();

        assert_eq!(data, decoded);
    }

    #[test]
    fn accumulate_cost_empty_test() {
        let scorer = Scorer::default();
        assert_eq!(scorer.accumulate_cost(&[], &[]), 0);
    }

    #[test]
    fn deserialize_invalid_bytes_should_fail() {
        let invalid_bytes = vec![0u8; 4];
        assert!(rkyv::from_bytes::<Scorer, Error>(&invalid_bytes).is_err());
    }
}