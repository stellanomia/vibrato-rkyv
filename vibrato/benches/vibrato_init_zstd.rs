use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use tar::Archive;
use xz2::bufread::XzDecoder;
use std::io::{self, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::fs::{self, File};

fn prepare_vibrato_dictionary(
    cache_dir: &Path,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let dict_dir = cache_dir;
    let dict_path = dict_dir.join("system.dic");
    let compressed_dict_path = dict_dir.join("unidic-cwj-3_1_1/system.dic.zst");

    if dict_path.exists() {
        println!("Vibrato Unidic dictionary found.");
        return Ok(compressed_dict_path);
    }

    if !compressed_dict_path.exists() {
        println!("Vibrato Unidic archive not found. Downloading...");
        fs::create_dir_all(dict_dir)?;

        let url = "https://github.com/daac-tools/vibrato/releases/download/v0.5.0/unidic-cwj-3_1_1.tar.xz";
        let response = reqwest::blocking::get(url)?.error_for_status()?;
        let tar_xz_bytes = response.bytes()?;

        println!("Decompressing and extracting archive...");
        let xz_decoder = XzDecoder::new(io::Cursor::new(tar_xz_bytes));
        let mut archive = Archive::new(xz_decoder);
        archive.unpack(cache_dir)?;
    }

    if compressed_dict_path.exists() {
        println!("Successfully created {}.", dict_path.display());
        Ok(compressed_dict_path)
    } else {
        Err("system.dic.zst not found after extraction.".into())
    }
}


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

fn bench_vibrato_dictionary_load(c: &mut Criterion) {
    let cache_dir = dirs::cache_dir()
        .expect("Failed to get cache directory")
        .join("vibrato-rkyv-assets/vibrato");

    let compressed_dict_path = prepare_vibrato_dictionary(&cache_dir)
        .expect("Failed to prepare vibrato dictionary.");

    if !compressed_dict_path.exists() {
        panic!("Dictionary file not found.");
    }

    let file_size = fs::metadata(&compressed_dict_path).unwrap().len();
    let mut group = c.benchmark_group("DictionaryLoad");
    group.throughput(Throughput::Bytes(file_size));

    group.sample_size(10);

    // vibrato (bincode)
    group.bench_function("vibrato/zstd/warm", |b| {
        let mut file = File::open(&compressed_dict_path).unwrap();
        b.iter(|| {
            file.seek(SeekFrom::Start(0)).unwrap();
            let mut decoder = zstd::Decoder::new(&file).unwrap();
            std::hint::black_box(vibrato::Dictionary::read(&mut decoder).unwrap());
        })
    });

    // vibrato (bincode, cold)
    group.bench_function("vibrato/zstd/cold", |b| {
        b.iter_with_setup(
            drop_caches,
            |_| {
                let file = File::open(&compressed_dict_path).unwrap();
                let mut decoder = zstd::Decoder::new(file).unwrap();
                std::hint::black_box(vibrato::Dictionary::read(&mut decoder).unwrap());
            },
        )
    });
    group.finish();
}

criterion_group!(benches, bench_vibrato_dictionary_load);
criterion_main!(benches);
