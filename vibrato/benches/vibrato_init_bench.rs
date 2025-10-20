use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use std::io::{Seek, SeekFrom};
use std::path::PathBuf;
use std::fs::{self, File};

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
    let dict_path = PathBuf::from("path/to/your/system.dic");

    if !dict_path.exists() {
        panic!("Dictionary file not found.");
    }

    let file_size = fs::metadata(&dict_path).unwrap().len();
    let mut group = c.benchmark_group("DictionaryLoad");
    group.throughput(Throughput::Bytes(file_size));

    group.sample_size(10);

    // vibrato (bincode)
    group.bench_function("vibrato/warm", |b| {
        let mut rdr = File::open(&dict_path).unwrap();
        b.iter(|| {
            rdr.seek(SeekFrom::Start(0)).unwrap();
            std::hint::black_box(vibrato::Dictionary::read(&rdr).unwrap());
        })
    });

    group.sample_size(10);

    // vibrato (bincode, cold)
    group.bench_function("vibrato/cold", |b| {
        b.iter_with_setup(
            drop_caches,
            |_| {
                let rdr = File::open(&dict_path).unwrap();
                std::hint::black_box(vibrato::Dictionary::read(&rdr).unwrap());
            },
        )
    });

    group.finish();
}

criterion_group!(benches, bench_vibrato_dictionary_load);
criterion_main!(benches);
