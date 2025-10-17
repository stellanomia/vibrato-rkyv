use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::io::{Seek, SeekFrom};
use std::path::PathBuf;
use std::fs::{self, File};

const UNIDIC_LEGACY_PATH_STR: &str = "path/to/your/system.dic";
const UNIDIC_RKYV_PATH_STR: &str = "path/to/your/system.dic";
const UNIDIC_RKYV_ZSTD_PATH_STR: &str = "path/to/your/system.dic.zst";

fn drop_caches() {
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        let _ = Command::new("sudo")
            .arg("sh")
            .arg("-c")
            .arg("echo 3 > /proc/sys/vm/drop_caches")
            .status();
    }
}

fn bench_dictionary_load(c: &mut Criterion) {
    let unidic_path = PathBuf::from(UNIDIC_LEGACY_PATH_STR);
    let unidic_rkyv_path = PathBuf::from(UNIDIC_RKYV_PATH_STR);
    let unidic_zstd_path = PathBuf::from(UNIDIC_RKYV_ZSTD_PATH_STR);

    if !unidic_path.exists() || !unidic_rkyv_path.exists() {
        panic!("Dictionary file not found. Set UNIDIC_PATH_STR and UNIDIC_RKYV_PATH_STR.");
    }

    let file_size = fs::metadata(&unidic_rkyv_path).unwrap().len();

    let mut group = c.benchmark_group("DictionaryLoad");
    group.throughput(Throughput::Bytes(file_size));

    group.sample_size(10);

    // vibrato (bincode)
    group.bench_function("vibrato/warm", |b| {
        let mut rdr = File::open(&unidic_path).unwrap();
        b.iter(|| {
            rdr.seek(SeekFrom::Start(0)).unwrap();
            std::hint::black_box(vibrato::Dictionary::read(&rdr).unwrap());
        })
    });

    group.sample_size(500);

    // vibrato-rkyv (from_path)
    group.bench_function("vibrato-rkyv/from_path/warm", |b| {
        let _ = fs::read(&unidic_rkyv_path).unwrap();
        b.iter(|| {
            std::hint::black_box(vibrato_rkyv::Dictionary::from_path(&unidic_rkyv_path).unwrap());
        })
    });

    // vibrato-rkyv (from_zstd, cached)
    group.bench_function("vibrato-rkyv/from_zstd/cached/warm", |b| {
        let _ = vibrato_rkyv::Dictionary::from_zstd(&unidic_zstd_path).unwrap();
        b.iter(|| {
            std::hint::black_box(vibrato_rkyv::Dictionary::from_zstd(&unidic_zstd_path).unwrap());
        })
    });

    group.sample_size(10);

    // vibrato (bincode)
    group.bench_function("vibrato/cold", |b| {
        b.iter_with_setup(
            drop_caches,
            |_| {
                let rdr = File::open(&unidic_path).unwrap();
                std::hint::black_box(vibrato::Dictionary::read(&rdr).unwrap());
            }
        )
    });

    // vibrato-rkyv (from_path)
    group.bench_function("vibrato-rkyv/from_path/cold", |b| {
        b.iter_with_setup(
            drop_caches,
            |_| {
                std::hint::black_box(vibrato_rkyv::Dictionary::from_path(&unidic_rkyv_path).unwrap());
            }
        )
    });

    group.bench_function("vibrato-rkyv/from_zstd/1st_run", |b| {
        b.iter_with_setup(
            || {
                let cache_dir = unidic_zstd_path.parent().unwrap().join("decompressed");
                if cache_dir.exists() {
                    fs::remove_dir_all(&cache_dir).unwrap();
                }
                drop_caches();
            },
            |_| {
                 std::hint::black_box(vibrato_rkyv::Dictionary::from_zstd(&unidic_zstd_path).unwrap());
            }
        )
    });


    group.finish();
}

criterion_group!(benches, bench_dictionary_load);
criterion_main!(benches);