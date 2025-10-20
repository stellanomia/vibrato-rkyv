#![cfg(feature = "download")]

use std::fmt;

/// Represents a preset dictionary that can be used without manual configuration.
///
/// These dictionaries are pre-serialized and available for convenience.
/// Currently, the available presets are `Ipadic` and `Unidic`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetDictionaryKind {
    /// MeCab IPADIC v2.7.0
    Ipadic,
    /// UniDic-cwj v3.1.1
    Unidic,
}

impl PresetDictionaryKind {
    pub(crate) fn meta(&self) -> &'static DictionaryMeta {
        match self {
            PresetDictionaryKind::Ipadic => &IPADIC,
            PresetDictionaryKind::Unidic => &UNIDIC,
        }
    }
}

pub(crate) static IPADIC: DictionaryMeta = DictionaryMeta {
    name: "mecab-ipadic",
    download_url: "https://github.com/stellanomia/vibrato-rkyv/releases/download/v0.6.2/mecab-ipadic.tar",
    sha256_hash_archive: "9e933a3149af4a0f8a6a36f44c37d95ef875416629bdc859c63265813be93b14",
    sha256_hash_comp_dict: "bc27ae4a2c717799dd1779f163fe22b33d048bfc4bc7635ecfb5441916754250",
};

pub(crate) static UNIDIC: DictionaryMeta = DictionaryMeta {
    name: "unidic-cwj",
    download_url: "https://github.com/stellanomia/vibrato-rkyv/releases/download/v0.6.2/unidic-cwj.tar",
    sha256_hash_archive: "2323b3bdcc50b5f8e00a6d729bacbf718f788905d4e300242201ed45c7f0b401",
    sha256_hash_comp_dict: "e3972b80a6ed45a40eb47063bdd30e7f3e051779b8df38ea191c8f2379c60130",
};

pub(crate) struct DictionaryMeta {
    pub name: &'static str,
    pub download_url: &'static str,
    pub sha256_hash_archive: &'static str,
    pub sha256_hash_comp_dict: &'static str,
}

impl fmt::Display for PresetDictionaryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PresetDictionaryKind::Ipadic => write!(f, "MeCab IPADIC v2.7.0"),
            PresetDictionaryKind::Unidic => write!(f, "UniDic-cwj v3.1.1"),
        }
    }
}
