use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use tempfile::TempDir;
use vibrato_rkyv::dictionary::GLOBAL_CACHE_DIR;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use vibrato_rkyv::dictionary::{CacheStrategy, LoadMode, PresetDictionaryKind};
use vibrato_rkyv::Dictionary;

struct BencherContext {
    _volatile_run_dir: TempDir,
    zstd_path: PathBuf,
    dict_path: PathBuf,
    local_cache_dir: PathBuf,
}

impl BencherContext {
    fn new() -> Self {
        println!("Setting up benchmark context...");

        let preset = PresetDictionaryKind::Unidic;
        let permanent_asset_dir = Self::permanent_asset_dir().join(preset.name());
        let permanent_zstd_path = permanent_asset_dir.join("system.dic.zst");

        let permanent_zstd_path = if !permanent_zstd_path.exists() {
            println!("Permanent asset not found. Downloading to {:?}", permanent_asset_dir);
            Dictionary::download_dictionary(preset, &permanent_asset_dir)
                .expect("Failed to download dictionary")
        } else {
            println!("Using permanent asset from: {:?}", permanent_zstd_path);
            permanent_zstd_path
        };

        let volatile_run_dir = tempfile::tempdir().expect("Failed to create volatile run directory");
        let zstd_path = volatile_run_dir.path().join("system.dic.zst");

        println!("Copying asset to volatile directory: {:?}", zstd_path);
        fs::copy(&permanent_zstd_path, &zstd_path)
            .expect("Failed to copy asset to volatile directory");

        let dict_path = zstd_path.with_extension("");
        println!("Decompressing dictionary to {:?}", dict_path);
        Dictionary::decompress_zstd(&zstd_path, &dict_path)
            .expect("Failed to decompress dictionary");

        let local_cache_dir = zstd_path.parent().unwrap().join(".cache");

        if !dict_path.exists() {
            unreachable!()
        }

        println!("Setup complete!");
        Self {
            _volatile_run_dir: volatile_run_dir,
            zstd_path,
            dict_path,
            local_cache_dir,
        }
    }

    fn permanent_asset_dir() -> PathBuf {
        let dir = dirs::cache_dir()
            .expect("Could not determine cache dir")
            .join("vibrato-rkyv-assets");
        fs::create_dir_all(&dir).expect("Failed to create permanent asset directory");
        dir
    }


    fn drop_os_caches(&self) {
        #[cfg(target_os = "linux")]
        {
            let status = Command::new("sudo")
                .arg("sh")
                .arg("-c")
                .arg("sync; echo 3 > /proc/sys/vm/drop_caches")
                .status();
            if !status.is_ok_and(|s| s.success()) {
                eprintln!("Warning: Failed to drop OS caches. Cold benchmarks may be inaccurate.");
            }
        }
    }

    fn clear_vibrato_caches(&self) {
        if self.local_cache_dir.exists() {
            fs::remove_dir_all(&self.local_cache_dir).unwrap();
        }

        if let Some(global_cache_dir) = GLOBAL_CACHE_DIR.as_ref()
            && global_cache_dir.exists() {
                fs::remove_dir_all(global_cache_dir).unwrap();
            }
    }
}

fn bench_vibrato_rkyv_dictionary_load(c: &mut Criterion) {
    let ctx = BencherContext::new();

    let file_size = fs::metadata(&ctx.dict_path).unwrap().len();
    let mut group = c.benchmark_group("DictionaryLoad");
    group.throughput(Throughput::Bytes(file_size));

    group.sample_size(50);
    group.bench_function("vibrato-rkyv/from_path/warm", |b| {
        let _ = fs::read(&ctx.dict_path).unwrap();
        let _ = Dictionary::from_path(&ctx.dict_path, LoadMode::TrustCache).unwrap();
        b.iter(|| std::hint::black_box(Dictionary::from_path(&ctx.dict_path, LoadMode::TrustCache).unwrap()))
    });

    group.bench_function("vibrato-rkyv/from_path_unchecked/warm", |b| {
        let _ = fs::read(&ctx.dict_path).unwrap();
        let _ = Dictionary::from_path(&ctx.dict_path, LoadMode::TrustCache).unwrap();
        b.iter(|| std::hint::black_box(unsafe { Dictionary::from_path_unchecked(&ctx.dict_path) }.unwrap()))
    });

    group.sample_size(30);
    group.bench_function("vibrato-rkyv/from_path/cold", |b| {
        b.iter_with_setup(
            || {
                ctx.drop_os_caches();
            },
            |_| Dictionary::from_path(&ctx.dict_path, LoadMode::TrustCache).unwrap(),
        )
    });

    group.sample_size(10);
    group.bench_function("vibrato-rkyv/from_path/1st_run", |b| {
        b.iter_with_setup(
            || {
                ctx.clear_vibrato_caches();
                ctx.drop_os_caches();
            },
            |_| Dictionary::from_path(&ctx.dict_path, LoadMode::TrustCache).unwrap(),
        )
    });

    group.sample_size(30);
    group.bench_function("vibrato-rkyv/from_zstd/cached/warm", |b| {
        let _ = Dictionary::from_zstd(&ctx.zstd_path, CacheStrategy::Local).unwrap();
        b.iter(|| Dictionary::from_zstd(&ctx.zstd_path, CacheStrategy::Local).unwrap())
    });


    group.sample_size(10);
    group.bench_function("vibrato-rkyv/from_zstd/cold", |b| {
        b.iter_with_setup(
            || {
                ctx.drop_os_caches();
            },
            |_| Dictionary::from_zstd(&ctx.zstd_path, CacheStrategy::Local).unwrap(),
        )
    });

    group.bench_function("vibrato-rkyv/from_zstd/1st_run", |b| {
        b.iter_with_setup(
            || {
                ctx.clear_vibrato_caches();
                ctx.drop_os_caches();
            },
            |_| Dictionary::from_zstd(&ctx.zstd_path, CacheStrategy::Local).unwrap(),
        )
    });

    group.finish();
}

criterion_group!(benches, bench_vibrato_rkyv_dictionary_load);
criterion_main!(benches);