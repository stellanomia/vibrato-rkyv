use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tempfile::{tempdir, TempDir};

use vibrato_rkyv::dictionary::{CacheStrategy, PresetDictionaryKind, GLOBAL_CACHE_DIR};
use vibrato_rkyv::{Dictionary, LoadMode};

struct GlobalTestResources {
    rkyv_dict_path: PathBuf,
    legacy_dict_path: PathBuf,
}

impl GlobalTestResources {
    fn new() -> Self {
        println!("Setting up global test resources for the first time...");

        let permanent_asset_dir = dirs::cache_dir()
            .expect("Could not determine cache dir")
            .join("vibrato-rkyv-assets");
        fs::create_dir_all(&permanent_asset_dir).expect("Failed to create permanent asset directory");

        let rkyv_dict_path = Self::download_if_not_exists(
            &permanent_asset_dir,
            PresetDictionaryKind::Ipadic,
        );

        let legacy_dict_path = {
            #[cfg(feature = "legacy")] {
                Self::download_if_not_exists(
                    &permanent_asset_dir,
                    PresetDictionaryKind::BccwjUnidicExtractedCompact,
                )
            }
            #[cfg(not(feature = "legacy"))] {
                PathBuf::new()
            }
        };

        println!("Global test resources are ready.");
        Self { rkyv_dict_path, legacy_dict_path }
    }

    fn download_if_not_exists(
        asset_dir: &Path,
        kind: PresetDictionaryKind,
    ) -> PathBuf {
        let preset_dir = asset_dir.join(kind.name());
        fs::create_dir_all(&preset_dir).unwrap();

        let expected_path = preset_dir.join("system.dic.zst");

        if !expected_path.exists() {
            println!("Downloading {} dictionary to {:?}", kind.name(), preset_dir);
            Dictionary::download_dictionary(kind, &preset_dir)
                .unwrap_or_else(|e| panic!("Failed to download {} dictionary: {}", kind.name(), e))
        } else {
            println!("Using cached asset: {:?}", expected_path);
            expected_path
        }
    }
}

static GLOBAL_RESOURCES: OnceLock<GlobalTestResources> = OnceLock::new();

fn global_resources() -> &'static GlobalTestResources {
    GLOBAL_RESOURCES.get_or_init(GlobalTestResources::new)
}

pub struct TestEnv {
    _temp_dir: TempDir,
    pub work_dir: PathBuf,
    pub rkyv_zst_path: PathBuf,
    pub legacy_zst_path: PathBuf,
}

impl TestEnv {
    pub fn new() -> Self {
        let global_resources = global_resources();
        let temp_dir = tempdir().expect("Failed to create a temporary directory");
        let work_dir = temp_dir.path().to_path_buf();

        let rkyv_zst_path = work_dir.join("system.dic.zst");
        fs::copy(&global_resources.rkyv_dict_path, &rkyv_zst_path).unwrap();

        let legacy_zst_path = {
            if global_resources.legacy_dict_path.exists() {
                let path = work_dir.join("legacy_system.dic.zst");
                fs::copy(&global_resources.legacy_dict_path, &path).unwrap();
                path
            } else {
                PathBuf::new()
            }
        };

        Self {
            _temp_dir: temp_dir,
            work_dir,
            rkyv_zst_path,
            legacy_zst_path,
        }
    }

    fn clear_vibrato_caches(&self) {
        let local_cache = self.work_dir.join(".cache");
        if local_cache.exists() {
            fs::remove_dir_all(local_cache).unwrap();
        }
        if let Some(global_cache_dir) = GLOBAL_CACHE_DIR.as_ref()
            && global_cache_dir.exists() {
                fs::remove_dir_all(global_cache_dir).unwrap();
            }
    }
}

impl Default for TestEnv {
    fn default() -> Self {
        TestEnv::new()
    }
}

#[test]
fn test_from_zstd_rkyv_creates_local_cache() {
    let env = TestEnv::new();
    env.clear_vibrato_caches();

    let dict = Dictionary::from_zstd(&env.rkyv_zst_path, CacheStrategy::Local).unwrap();

    let expected_local_cache_dir = env.work_dir.join(".cache");
    assert!(expected_local_cache_dir.exists());
    assert!(expected_local_cache_dir.read_dir().unwrap().next().is_some());

    assert!(matches!(dict, Dictionary::Archived(_)));
}

#[test]
#[cfg(feature = "legacy")]
fn test_from_zstd_legacy_converts_and_caches_as_rkyv() {
    let env = TestEnv::new();
    env.clear_vibrato_caches();

    let cache_dir = env.work_dir.join(".cache");
    let dict_legacy = Dictionary::from_zstd_with_options(&env.legacy_zst_path, &cache_dir, true).unwrap();
    assert!(matches!(dict_legacy, Dictionary::Owned { .. }));

    assert!(cache_dir.exists());
    let cached_files: Vec<_> = fs::read_dir(&cache_dir).unwrap().map(|r| r.unwrap().path()).collect();
    assert_eq!(cached_files.len(), 2);

    let dict_rkyv_from_cache = Dictionary::from_zstd(&env.legacy_zst_path, CacheStrategy::Local).unwrap();
    assert!(matches!(dict_rkyv_from_cache, Dictionary::Archived(_)));
}

#[test]
fn test_from_path_trustcache_flow() {
    let env = TestEnv::new();
    env.clear_vibrato_caches();

    let dic_path = env.work_dir.join("test.dic");
    Dictionary::decompress_zstd(&env.rkyv_zst_path, &dic_path).unwrap();

    let _ = Dictionary::from_path(&dic_path, LoadMode::TrustCache).unwrap();
    let global_cache = GLOBAL_CACHE_DIR.as_ref().unwrap();
    assert!(global_cache.exists() && global_cache.read_dir().unwrap().next().is_some());

    {
        let dict_hit = Dictionary::from_path(&dic_path, LoadMode::TrustCache).unwrap();
        assert!(matches!(dict_hit, Dictionary::Archived(_)));
    }

    fs::write(&dic_path, b"corrupted data").unwrap();
    let result_corrupted = Dictionary::from_path(&dic_path, LoadMode::TrustCache);
    assert!(result_corrupted.is_err());
}

#[test]
fn test_from_path_validate_mode() {
    let env = TestEnv::new();
    env.clear_vibrato_caches();

    let dic_path = env.work_dir.join("test.dic");
    Dictionary::decompress_zstd(&env.rkyv_zst_path, &dic_path).unwrap();

    let dict = Dictionary::from_path(&dic_path, LoadMode::Validate).unwrap();

    assert!(matches!(dict, Dictionary::Archived(_)));
}