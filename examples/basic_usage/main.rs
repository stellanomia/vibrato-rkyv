use std::error::Error;
use std::fs;
use std::path::PathBuf;

use vibrato_rkyv::{Dictionary, Tokenizer};
use vibrato_rkyv::dictionary::PresetDictionaryKind;

fn main() -> Result<(), Box<dyn Error>> {
    // This example uses a subdirectory in the system's standard cache location.
    let mut cache_dir = dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".cache"));
    cache_dir.push("vibrato-rkyv-examples");
    fs::create_dir_all(&cache_dir)?;

    println!("Cache directory: {}", cache_dir.display());

    // `from_preset_with_download` handles downloading, checksum verification,
    // and caching. The first run will download the dictionary, but subsequent
    // runs will load the cache instantly.
    //
    // Available presets without any feature flags:
    // - Ipadic: MeCab IPADIC v2.7.0
    // - Unidic: UniDic-cwj v3.1.1
    //
    // Other dictionaries are available with the `legacy` feature flag.
    println!("Loading the IPADIC preset dictionary. This may take a moment on the first run...");
    let dict = Dictionary::from_preset_with_download(
        PresetDictionaryKind::Ipadic,
        &cache_dir,
    )?;
    println!("Dictionary loaded successfully.");

    // The tokenizer is created from the dictionary.
    //
    // Note: When using Dictionary::from_zstd with the legacy feature,
    // this function's move semantics may cause the current thread
    // to block and wait for a background caching thread to finish when the tokenizer is dropped.
    let tokenizer = Tokenizer::new(dict);

    let mut worker = tokenizer.new_worker();

    let text = "あなたは猫が好きですか？";
    println!("\nTokenizing the text: \"{}\"", text);
    worker.reset_sentence(text);
    worker.tokenize();

    println!("\nTokenization Result:");
    for token in worker.token_iter() {
        println!("{}\t{}", token.surface(), token.feature());
    }

    Ok(())
}
