use std::fs;
use std::{error::Error, path::PathBuf};
use std::path::Path;

use vibrato_rkyv::{Dictionary, LoadMode, Tokenizer};

/// This example demonstrates how to load a local dictionary file using
/// different `LoadMode`s: `Validate` for guaranteed safety, and `TrustCache`
/// for speed on subsequent loads.
///
/// ## Prerequisites
///
/// To run this example, you must have a pre-compiled dictionary file (e.g., `system.dic`)
/// available locally. There are several ways to get one:
///
/// ### Option 1: Download from Releases
///
/// You can download a pre-compiled dictionary directly from the project's
/// GitHub Releases page:
/// > https://github.com/stellanomia/vibrato-rkyv/releases
///
/// Download a `.tar` file (e.g., `mecab-ipadic.tar`), extract it, and you will
/// find the `.dic.zst` file inside.
///
/// ### Option 2: Download Programmatically
///
/// You can create a small helper script that uses the `download_dictionary` API
/// to fetch a preset dictionary.
///
/// ```no_run
/// use vibrato_rkyv::{Dictionary, dictionary::PresetDictionaryKind};
/// use std::path::Path;
///
/// fn prepare_dictionary() -> Result<(), Box<dyn Error>> {
///     let cache_dir = Path::new("./dictionary_cache");
///     // This downloads the compressed dictionary (.zst)
///     let zst_path = Dictionary::download_dictionary(PresetDictionaryKind::Ipadic, cache_dir)?;
///     // This decompresses and validates it, creating the .dic file
///     let _ = Dictionary::from_zstd(zst_path)?;
///     println!("Dictionary is ready in ./dictionary_cache/decompressed/");
///     Ok(())
/// }
/// ```
///
/// ### Option 3: Compile from Source (Advanced)
///
/// For full control, you can compile a dictionary from source CSV files using
/// the `compiler` tool in this workspace:
///   `cargo run --release -p compiler -- build ... --sysdic-out system.dic`
///
/// Once you have the dictionary file, place it in the root of this workspace,
/// or modify the `DICT_PATH` constant below.
fn main() -> Result<(), Box<dyn Error>> {
    const ZSTD_DICT_PATH: &str = "system.dic.zst";

    println!("--- Manual Dictionary Loading Example ---");

    if !Path::new(ZSTD_DICT_PATH).exists() {
        eprintln!("Error: Compressed dictionary file not found at '{}'", ZSTD_DICT_PATH);
        eprintln!("See comments in `vibrato/examples/manual_load.rs` for setup instructions.");
        return Err("Dictionary file missing".into());
    }

    let text = "あなたは猫が好きですか？";

    // `from_zstd`:
    // It automatically handles decompression and caching to a `decompressed`
    // subdirectory next to the source file.
    println!("\n1. Loading with `from_zstd`");
    let _dict_zstd = Dictionary::from_zstd(ZSTD_DICT_PATH)?;
    println!("Dictionary loaded from '{}'. Check for a 'decompressed' directory nearby.", ZSTD_DICT_PATH);

    // (Tokenization is the same for all, so we'll show it once at the end)

    // Clean up the cache created by `from_zstd` for the next step.
    let default_cache_dir = Path::new(ZSTD_DICT_PATH).parent().unwrap().join("decompressed");
    if default_cache_dir.exists() {
        fs::remove_dir_all(&default_cache_dir)?;
    }

    // `from_zstd_with_options`:
    // Here we use it to decompress the dictionary into a predictable location
    // that we can then use for the `from_path` examples.
    println!("\n2. Setting up for `from_path` using `from_zstd_with_options`");
    let setup_cache_dir = PathBuf::from("./manual_load_cache");
    let _ = Dictionary::from_zstd_with_options(ZSTD_DICT_PATH, &setup_cache_dir, false, false)?;

    // Now, we have the decompressed `.dic` file ready in our controlled location.
    let dic_path = setup_cache_dir.join(Path::new(ZSTD_DICT_PATH).file_stem().unwrap());
    println!("Decompressed dictionary is ready at: {}", dic_path.display());

    // `from_path:
    println!("\n3. Loading with `from_path`");
    println!("\n3a. Using LoadMode::Validate");

    // LoadMode::Validate: Always Safe
    let _dict_validate = Dictionary::from_path(&dic_path, LoadMode::Validate)?;
    println!("Dictionary loaded safely with validation.");


    println!("\n3b. Using LoadMode::TrustCache");
    println!("(First run with this mode creates a cache file: {}.sha256)", dic_path.display());

    // LoadMode::TrustCache: Fast on Subsequent Loads
    let dict_trust_cache = Dictionary::from_path(&dic_path, LoadMode::TrustCache)?;
    println!("Dictionary loaded via TrustCache mode.");


    // Now let's see the tokenizer in action with the final loaded dictionary
    println!("\n--- Tokenization Example ---");
    let tokenizer_final = Tokenizer::new(dict_trust_cache);
    let mut worker_final = tokenizer_final.new_worker();
    worker_final.reset_sentence(text);
    worker_final.tokenize();
    for token in worker_final.token_iter() {
        println!("  {}\t{}", token.surface(), token.feature());
    }

    if setup_cache_dir.exists() {
        fs::remove_dir_all(&setup_cache_dir)?;
    }

    Ok(())
}
