#![cfg(feature = "download")]

use std::fmt;

/// Represents a preset dictionary that can be used without manual configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresetDictionaryKind {
    /// MeCab IPADIC v2.7.0
    Ipadic,
    /// UniDic-cwj v3.1.1
    Unidic,

    #[cfg(feature = "legacy")]
    /// UniDic-cwj v3.1.1 + Compact
    UnidicCompact,

    #[cfg(feature = "legacy")]
    /// UniDic-cwj v3.1.1 + Compact-dual
    UnidicCompactDual,

    #[cfg(feature = "legacy")]
    /// UniDic-cwj (trained BCCWJ) v3.1.1
    BccwjUnidic,

    #[cfg(feature = "legacy")]
    /// UniDic-cwj (trained BCCWJ) v3.1.1 + Compact
    BccwjUnidicCompact,

    #[cfg(feature = "legacy")]
    /// UniDic-cwj (trained BCCWJ) v3.1.1 + Compact-dual
    BccwjUnidicCompactDual,

    #[cfg(feature = "legacy")]
    /// UniDic-cwj (trained BCCWJ) v3.1.1 + Extracted POS and pronunciation features + Compact
    BccwjUnidicExtractedCompact,

    #[cfg(feature = "legacy")]
    /// UniDic-cwj (trained BCCWJ) v3.1.1 + Extracted POS and pronunciation features + Compact-dual
    BccwjUnidicExtractedCompactDual,
}

impl PresetDictionaryKind {
    pub(crate) fn meta(&self) -> &'static DictionaryMeta {
        use PresetDictionaryKind::*;

        match self {
            Ipadic => &IPADIC,
            Unidic => &UNIDIC,

            #[cfg(feature = "legacy")]
            UnidicCompact => &UNIDIC_COMPACT,

            #[cfg(feature = "legacy")]
            UnidicCompactDual => &UNIDIC_COMPACT_DUAL,

            #[cfg(feature = "legacy")]
            BccwjUnidic => &BCCWJ_UNIDIC,

            #[cfg(feature = "legacy")]
            BccwjUnidicCompact => &BCCWJ_UNIDIC_COMPACT,

            #[cfg(feature = "legacy")]
            BccwjUnidicCompactDual => &BCCWJ_UNIDIC_COMPACT_DUAL,

            #[cfg(feature = "legacy")]
            BccwjUnidicExtractedCompact => &BCCWJ_UNIDIC_EXTRACTED_COMPACT,

            #[cfg(feature = "legacy")]
            BccwjUnidicExtractedCompactDual => &BCCWJ_UNIDIC_EXTRACTED_COMPACT_DUAL,
        }
    }

    pub fn name(&self) -> &'static str {
        self.meta().name
    }
}

use FileType::*;

pub(crate) static IPADIC: DictionaryMeta = DictionaryMeta {
    name: "mecab-ipadic",
    file_type: Tar,
    download_url: "https://github.com/stellanomia/vibrato-rkyv/releases/download/v0.6.2/mecab-ipadic.tar",
    sha256_hash_archive: "9e933a3149af4a0f8a6a36f44c37d95ef875416629bdc859c63265813be93b14",
    sha256_hash_comp_dict: "bc27ae4a2c717799dd1779f163fe22b33d048bfc4bc7635ecfb5441916754250",
};

pub(crate) static UNIDIC: DictionaryMeta = DictionaryMeta {
    name: "unidic-cwj",
    file_type: Tar,
    download_url: "https://github.com/stellanomia/vibrato-rkyv/releases/download/v0.6.2/unidic-cwj.tar",
    sha256_hash_archive: "2323b3bdcc50b5f8e00a6d729bacbf718f788905d4e300242201ed45c7f0b401",
    sha256_hash_comp_dict: "e3972b80a6ed45a40eb47063bdd30e7f3e051779b8df38ea191c8f2379c60130",
};

#[cfg(feature = "legacy")]
pub(crate) static UNIDIC_COMPACT: DictionaryMeta = DictionaryMeta {
    name: "unidic-cwj+compact",
    file_type: TarXz,
    download_url: "https://github.com/daac-tools/vibrato/releases/download/v0.5.0/unidic-cwj-3_1_1+compact.tar.xz",
    sha256_hash_archive: "9bd032f29424daaf90a92d2835961b2f3a3c0a4cf15e2092c63cd356c2e9b4d2",
    sha256_hash_comp_dict: "487ca64b39a31af2f054d905d333a82d0ec0872530d3610342b3c56b0b4b4ad0",
};

#[cfg(feature = "legacy")]
pub(crate) static UNIDIC_COMPACT_DUAL: DictionaryMeta = DictionaryMeta {
    name: "unidic-cwj+compact-dual",
    file_type: TarXz,
    download_url: "https://github.com/daac-tools/vibrato/releases/download/v0.5.0/unidic-cwj-3_1_1+compact-dual.tar.xz",
    sha256_hash_archive: "2d3329476588b18415b4796556a1e9cf6cc6071299fd3976ee4298ac88357d45",
    sha256_hash_comp_dict: "132c75f8e64b255bf2787122292ac3839d8f0c8590d9e9ae2f230a0a378fd172",
};

#[cfg(feature = "legacy")]
pub(crate) static BCCWJ_UNIDIC: DictionaryMeta = DictionaryMeta {
    name: "bccwj-suw+unidic-cwj",
    file_type: TarXz,
    download_url: "https://github.com/daac-tools/vibrato/releases/download/v0.5.0/bccwj-suw+unidic-cwj-3_1_1.tar.xz",
    sha256_hash_archive: "668aa982b64dfc719f8a4cedfef18f09108b27afe0599eb2fe1351d4790529bb",
    sha256_hash_comp_dict: "71d77e3a4d4d029e1edc34da2941a947667a89cac951cfdf6bccd34dce4c160f",
};

#[cfg(feature = "legacy")]
pub(crate) static BCCWJ_UNIDIC_COMPACT: DictionaryMeta = DictionaryMeta {
    name: "bccwj-suw+unidic-cwj+compact",
    file_type: TarXz,
    download_url: "https://github.com/daac-tools/vibrato/releases/download/v0.5.0/bccwj-suw+unidic-cwj-3_1_1+compact.tar.xz",
    sha256_hash_archive: "143e3704658a41db1f6e236ba0c8a062dc370578398d1343b6aeb7252783a3f4",
    sha256_hash_comp_dict: "78c25cea4a7bb8dcab3f5117f2957923df83edb0bf44fafdb3e98b5af825779d",
};

#[cfg(feature = "legacy")]
pub(crate) static BCCWJ_UNIDIC_COMPACT_DUAL: DictionaryMeta = DictionaryMeta {
    name: "bccwj-suw+unidic-cwj+compact-dual",
    file_type: TarXz,
    download_url: "https://github.com/daac-tools/vibrato/releases/download/v0.5.0/bccwj-suw+unidic-cwj-3_1_1+compact-dual.tar.xz",
    sha256_hash_archive: "4d45281de92190e214cf396e1d38e82c1262d24b3c576f6bdf84e9c6d8959760",
    sha256_hash_comp_dict: "af9c934fc831506aebcb68c11f446c8625a9cd0cd46914d4c16d2940e4f9d69b",
};

#[cfg(feature = "legacy")]
pub(crate) static BCCWJ_UNIDIC_EXTRACTED_COMPACT: DictionaryMeta = DictionaryMeta {
    name: "bccwj-suw+unidic-cwj-extracted+compact",
    file_type: TarXz,
    download_url: "https://github.com/daac-tools/vibrato/releases/download/v0.5.0/bccwj-suw+unidic-cwj-3_1_1-extracted+compact.tar.xz",
    sha256_hash_archive: "28862fae8727f585271ea31ba7ec2fb4878711bea2377b3260ee179ce8e77bcc",
    sha256_hash_comp_dict: "2f99875d94e309f112550c00956ab13c7cad1da5979f10e84680288d910de9dc",
};

#[cfg(feature = "legacy")]
pub(crate) static BCCWJ_UNIDIC_EXTRACTED_COMPACT_DUAL: DictionaryMeta = DictionaryMeta {
    name: "bccwj-suw+unidic-cwj-extracted+compact-dual",
    file_type: TarXz,
    download_url: "https://github.com/daac-tools/vibrato/releases/download/v0.5.0/bccwj-suw+unidic-cwj-3_1_1-extracted+compact-dual.tar.xz",
    sha256_hash_archive: "667c4ea3385db13271d546a4c38e189479c0f78a7d5d7b276b5a39c981e1ff7c",
    sha256_hash_comp_dict: "8b3539626d14a7393c95e46704c213cf01cb8a1d8bf42be9dfdfbabbcdd1abfb",
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DictionaryMeta {
    pub name: &'static str,
    pub file_type: FileType,
    pub download_url: &'static str,
    pub sha256_hash_archive: &'static str,
    pub sha256_hash_comp_dict: &'static str,
}

impl fmt::Display for PresetDictionaryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FileType {
    Tar,
    #[allow(unused)]
    TarXz,
}