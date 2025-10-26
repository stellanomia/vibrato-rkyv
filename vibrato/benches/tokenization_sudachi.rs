use std::fs::File;
use std::{fs, io};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use sudachi::analysis::stateful_tokenizer::StatefulTokenizer;
use sudachi::config::Config;
use sudachi::dic::dictionary::JapaneseDictionary;
use sudachi::dic::subset::InfoSubset;
use sudachi::prelude::{Mode, MorphemeList};
use zip::ZipArchive;

const CORPUS: &str = include_str!("./resources/waganeko.txt");

fn prepare_sudachi_dictionary(
    resource_dir: &Path,
    dict_name: &str, // "small", "core", "full"
) -> Result<(), Box<dyn std::error::Error>> {
    let dict_path = resource_dir.join(format!("system_{}.dic", dict_name));
    if dict_path.exists() {
        println!("Sudachi {} dictionary found.", dict_name);
        return Ok(());
    }

    println!("Sudachi {} dictionary not found. Downloading...", dict_name);
    fs::create_dir_all(resource_dir)?;


    let tag = "20250828";
    let version = "20250825";
    let url = format!(
        "https://github.com/WorksApplications/SudachiDict/releases/download/v{}/sudachi-dictionary-{}-{}.zip",
        tag, version, dict_name
    );
    let response = reqwest::blocking::get(url)?;
    let response = response.error_for_status()?;
    let zip_bytes = response.bytes()?;

    let reader = io::Cursor::new(zip_bytes);
    let mut archive = ZipArchive::new(reader)?;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let file_name = file.mangled_name().to_string_lossy().to_string();
        if file_name.ends_with(".dic") {
            println!("Extracting {} to {}", file_name, dict_path.display());
            let mut outfile = File::create(&dict_path)?;
            io::copy(&mut file, &mut outfile)?;
            return Ok(());
        }
    }

    Err("No .dic file found in the downloaded zip archive.".into())
}

fn benchmark_sudachi_preset(
    c: &mut Criterion,
    dict_name: &str, // "small", "core", or "full"
    cache_dir: &Path,
    lines: &[&str],
) {
    println!("Preparing Sudachi {} dictionary...", dict_name);

    prepare_sudachi_dictionary(cache_dir, dict_name)
        .unwrap_or_else(|e| panic!("Failed to prepare Sudachi {} dictionary: {}", dict_name, e));

    let dict_path = cache_dir.join(format!("system_{}.dic", dict_name));
    let config = Config::new(None, None, Some(dict_path)).unwrap();
    let dict = Arc::new(JapaneseDictionary::from_cfg(&config).unwrap());
    println!("Sudachi {} ready.", dict_name);

    let total_bytes: usize = CORPUS.len();

    let mut group = c.benchmark_group(format!("Tokenization Speed (Sudachi-{})", dict_name));
    group.throughput(Throughput::Bytes(total_bytes as u64));
    group.warm_up_time(Duration::from_secs(3));
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(20);

    for &mode in &[Mode::A, Mode::B, Mode::C] {
        group.bench_function(BenchmarkId::new(format!("{:?}", mode), "Corpus"), |b| {
            b.iter_with_setup(
                || {
                    let mut tokenizer = StatefulTokenizer::new(dict.as_ref(), mode);
                    tokenizer.set_subset(InfoSubset::empty());
                    let morphemes = MorphemeList::empty(dict.as_ref());
                    (tokenizer, morphemes)
                },
                |(mut tokenizer, mut morphemes)| {
                    for line in lines {
                        tokenizer.reset().push_str(line);
                        tokenizer.do_tokenize().unwrap();
                        morphemes.collect_results(&mut tokenizer).unwrap();
                    }
                },
            );
        });
    }

    group.finish();
}

fn bench_all_sudachi_presets(c: &mut Criterion) {
    let cache_dir = dirs::cache_dir()
        .expect("Failed to get cache directory")
        .join("sudachi-rs-bench-cache");

    let lines: &[&str] = &CORPUS.lines().collect::<Vec<&str>>();

    benchmark_sudachi_preset(c, "core", &cache_dir, lines);
}

criterion_group!(benches, bench_all_sudachi_presets);
criterion_main!(benches);
