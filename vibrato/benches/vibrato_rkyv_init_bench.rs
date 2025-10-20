use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use vibrato_rkyv::Dictionary;
use vibrato_rkyv::dictionary::PresetDictionaryKind;
use std::path::Path;
use std::fs;

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

fn bench_vibrato_rkyv_dictionary_load(c: &mut Criterion) {
    let cache_dir = dirs::cache_dir().unwrap().join("vibrato-rkyv");
    println!("downloading...");
    let dict_zstd_path = Dictionary::download_dictionary(
        PresetDictionaryKind::Unidic,
        &cache_dir,
    ).unwrap();
    println!("completed!");
    let _ = Dictionary::from_zstd(&dict_zstd_path);
    let dict_rkyv = &cache_dir.join("decompressed").join(dict_zstd_path.file_stem().unwrap());
    let dict_rkyv_path = Path::new(dict_rkyv);

    if !dict_rkyv_path.exists() {
        panic!("Dictionary file not found.");
    }

    let file_size = fs::metadata(dict_rkyv_path).unwrap().len();
    let mut group = c.benchmark_group("DictionaryLoad");
    group.throughput(Throughput::Bytes(file_size));

    group.sample_size(500);

    // vibrato-rkyv (from_path)
    group.bench_function("vibrato-rkyv/from_path/warm", |b| {
        let _ = fs::read(dict_rkyv_path).unwrap();
        b.iter(|| {
            std::hint::black_box(vibrato_rkyv::Dictionary::from_path(dict_rkyv_path).unwrap());
        })
    });

    // vibrato-rkyv (from_zstd, cached)
    group.bench_function("vibrato-rkyv/from_zstd/cached/warm", |b| {
        let _ = vibrato_rkyv::Dictionary::from_zstd(&dict_zstd_path).unwrap();
        b.iter(|| {
            std::hint::black_box(vibrato_rkyv::Dictionary::from_zstd(&dict_zstd_path).unwrap());
        })
    });

    group.sample_size(30);

    // vibrato-rkyv (from_path, cold)
    group.bench_function("vibrato-rkyv/from_path/cold", |b| {
        b.iter_with_setup(
            drop_caches,
            |_| {
                std::hint::black_box(vibrato_rkyv::Dictionary::from_path(dict_rkyv_path).unwrap());
            },
        )
    });

    group.sample_size(10);

    // vibrato-rkyv (from_zstd, 1st run)
    group.bench_function("vibrato-rkyv/from_zstd/1st_run", |b| {
        b.iter_with_setup(
            || {
                let cache_dir = dict_zstd_path.parent().unwrap().join("decompressed");
                if cache_dir.exists() {
                    fs::remove_dir_all(&cache_dir).unwrap();
                }
                drop_caches();
            },
            |_| {
                std::hint::black_box(vibrato_rkyv::Dictionary::from_zstd(&dict_zstd_path).unwrap());
            },
        )
    });

    group.finish();
}

criterion_group!(benches, bench_vibrato_rkyv_dictionary_load);
criterion_main!(benches);
