use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use vibrato_rkyv::{dictionary::PresetDictionaryKind, Dictionary, Tokenizer};

const CORPUS: &str = include_str!("./resources/waganeko.txt");

fn benchmark_preset(
    c: &mut Criterion,
    kind: PresetDictionaryKind,
    cache_dir: &Path,
    lines: &[&str],
) {
    let cache_dir = cache_dir.join(kind.name());
    println!("Preparing {} dictionary...", kind.name());

    let dict = Arc::new(
        Dictionary::from_preset_with_download(kind, cache_dir)
            .unwrap_or_else(|e| panic!("Failed to load {}: {}", kind.name(), e)),
    );
    println!("{} ready.", kind.name());

    let total_bytes: usize = CORPUS.len();

    let mut group = c.benchmark_group(format!("Tokenization Speed ({})", kind.name()));
    group.throughput(Throughput::Bytes(total_bytes as u64));
    group.warm_up_time(Duration::from_secs(3));
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(20);

    group.bench_function(BenchmarkId::new("Default", "Corpus"), |b| {
        b.iter_with_setup(
            || {
                let tokenizer = Tokenizer::from_shared_dictionary(dict.clone());
                tokenizer.new_worker()
            },
            |mut worker| {
                for line in lines {
                    worker.reset_sentence(line);
                    worker.tokenize();
                }
            },
        );
    });

    group.bench_function(BenchmarkId::new("MeCab-Compat", "Corpus"), |b| {
        b.iter_with_setup(
            || {
                let tokenizer = Tokenizer::from_shared_dictionary(dict.clone())
                    .ignore_space(true)
                    .unwrap()
                    .max_grouping_len(24);
                tokenizer.new_worker()
            },
            |mut worker| {
                for line in lines {
                    worker.reset_sentence(line);
                    worker.tokenize();
                }
            },
        );
    });

    group.finish();
}

fn bench_all_presets(c: &mut Criterion) {
    let cache_dir = dirs::cache_dir()
        .expect("Failed to get cache directory")
        .join("vibrato-rkyv-assets");

    let lines: &[&str] = &CORPUS.lines().collect::<Vec<&str>>();

    benchmark_preset(c, PresetDictionaryKind::Ipadic, &cache_dir, lines);
    benchmark_preset(c, PresetDictionaryKind::UnidicCwj, &cache_dir, lines);
    benchmark_preset(c, PresetDictionaryKind::BccwjUnidic, &cache_dir, lines);
    benchmark_preset(c, PresetDictionaryKind::BccwjUnidicCompactDual, &cache_dir, lines);
    benchmark_preset(c, PresetDictionaryKind::BccwjUnidicExtractedCompact, &cache_dir, lines);
    benchmark_preset(c, PresetDictionaryKind::BccwjUnidicExtractedCompactDual, &cache_dir, lines);
}

criterion_group!(benches, bench_all_presets);
criterion_main!(benches);