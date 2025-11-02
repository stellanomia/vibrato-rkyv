# 🎤 vibrato-rkyv: VIterbi-Based acceleRAted TOkenizer with rkyv

**Note:** This is a fork of the original [daac-tools/vibrato](https://github.com/daac-tools/vibrato) modified to use the `rkyv` serialization framework for significantly faster dictionary loading.

[![Crates.io](https://img.shields.io/crates/v/vibrato-rkyv)](https://crates.io/crates/vibrato-rkyv)
[![Documentation](https://docs.rs/vibrato-rkyv/badge.svg)](https://docs.rs/vibrato-rkyv)
[![Build Status](https://github.com/stellanomia/vibrato-rkyv/actions/workflows/rust.yml/badge.svg)](https://github.com/stellanomia/vibrato-rkyv/actions)

Vibrato is a fast implementation of tokenization (or morphological analysis) based on the Viterbi algorithm.

## Significantly Faster Dictionary Loading with `rkyv`

`vibrato-rkyv` utilizes the [`rkyv`](https://rkyv.org/) zero-copy deserialization framework to achieve a significant speedup in dictionary loading. By memory-mapping the dictionary file, it can be made available for use almost instantaneously.

The benchmark results below compare loading from both uncompressed and `zstd`-compressed files, demonstrating the performance difference.

CPU: Intel Core i7-14700  
OS: WSL2 (Ubuntu 24.04)  
Dictionary: UniDic-cwj v3.1.1 (approx. 700MB uncompressed dictionary binary)  
Source: The benchmark code is available in the [benches](./vibrato/benches) directory.  

### From Uncompressed File (`.dic`)

| Condition | Original `vibrato` (Read from stream) | `vibrato-rkyv` (Memory-mapped) | Speedup |
| :--- | :--- | :--- | :--- |
| Cold Start | ~42 s | ~1.1 ms | ~38,000x |
| Warm Start | ~34 s | ~3.0 μs | ~11,000,000x |

### From Zstd-Compressed File (`.dic.zst`)

| Condition | Original `vibrato` (Read from stream) | `vibrato-rkyv` (with caching) | Speedup |
| :--- | :--- | :--- | :--- |
| 1st Run (Cold) | ~4.6 s | ~1.3 s | ~3.5x |
| Subsequent Runs (Cache-hit) | ~4.5 s | ~6.5 μs | ~700,000x |

<small>*`vibrato-rkyv` automatically decompresses and caches the dictionary on the first run, using the memory-mapped cache for subsequent loads.*</small>

To take advantage of this performance, use the `Dictionary::from_path` or `Dictionary::from_zstd` methods:

```rust
use vibrato_rkyv::{Dictionary, LoadMode};

// Recommended for uncompressed dictionaries:
// Almost instantaneous loading via memory-mapping.
let dict_mmap = Dictionary::from_path("path/to/system.dic")?;

// Recommended for zstd-compressed dictionaries:
// Decompresses and caches on the first run, then uses memory-mapping.
let dict_zstd = Dictionary::from_zstd("path/to/system.dic.zst", LoadMode::TrustCache)?;
```

## Differences

The following summarizes key differences from the original implementation.

### Differences from Original `vibrato`

If you are migrating from the original `daac-tools/vibrato`, please note the following key changes:

- **Legacy Dictionary Support (with legacy feature):** `vibrato-rkyv` is designed for performance with its native `rkyv`-based dictionary format. However, to provide flexibility and allow users to leverage a wide range of dictionary assets, it also offers support for the `bincode`-based format used by the original `vibrato` when the `legacy` feature is enabled.  
This enables the use of valuable, pre-existing dictionaries that may only be available in the `bincode` format, such as those trained on proprietary corpora (e.g., BCCWJ).  
The library handles different formats:
  - `Dictionary::from_path()`: Transparently loads both uncompressed `rkyv` and `bincode` format dictionaries. It automatically detects the format based on the file's content.
  - `Dictionary::from_zstd()`: When given a Zstandard-compressed dictionary, it provides sophisticated, format-aware caching:
    - If the dictionary is in the `rkyv` format, it is decompressed and cached for near-instant, memory-mapped access on subsequent loads.
    - If the dictionary is in the `bincode` format, it is loaded directly into memory for immediate use. In the background, a process is started to convert it to the `rkyv` format and create a separate cache. This ensures that while the first load is operational, all future loads benefit from the high-speed `rkyv` cache.

This eliminates the need for manual conversion for most use cases. For users who prefer to convert dictionaries, the compiler transmute command is also available (see [Toolchain](#additional-improvements) below).

- **User Dictionaries Must Be Pre-compiled:** The `--user-dic` runtime option has been removed. User dictionaries must now be compiled into the system dictionary beforehand. This design choice supports the zero-copy, immutable model of `rkyv`.  
  However, this does not mean dictionaries are purely static. While you cannot modify a dictionary *after* it has been loaded, you can dynamically construct a dictionary in memory (e.g., using `SystemDictionaryBuilder`) and create a `Tokenizer` from it using `Dictionary::from_inner()`. This is useful for scenarios where dictionary contents are generated at runtime before tokenization begins.

- **New Recommended Loading APIs:** For maximum performance, use `Dictionary::from_path()` for uncompressed files and `Dictionary::from_zstd()` for `zstd`-compressed files. These methods leverage memory-mapping and caching for near-instantaneous loading. While `Dictionary::read()` is still available for generic readers, it is less efficient.

```rust
use vibrato_rkyv::{dictionary::LoadMode, Dictionary};

// Recommended: Zero-copy loading via memory-mapping.
let dict = Dictionary::from_path("path/to/system.dic", LoadMode::TrustCache)?;
```

### Additional Improvements

Beyond the core change to `rkyv` for faster loading, `vibrato-rkyv` includes several other significant enhancements over the original implementation:

* **Unified and Enhanced Toolchain (`compiler`)**  
  The `train`, `dictgen`, and `compile` executables have been consolidated into a single, more powerful `compiler` tool. This simplifies the dictionary creation workflow with a clear subcommand structure (`train`, `dictgen`, `build`). It also adds:
  * `full-build`: A convenient command to run the entire train-generate-build process in one go.
  * `transmute`: A utility to convert legacy `bincode`-formatted dictionaries from the original `vibrato` to the new `rkyv` format.

* **Flexible `Tokenizer`**  
  The `Tokenizer` API has been redesigned for better flexibility, resolving a long-standing design limitation ([upstream issue #99](https://github.com/daac-tools/vibrato/issues/99)).
  * It is now cheaply `Clone`-able (internally using `Arc<Dictionary>`).
  * New constructors like `Tokenizer::from_inner(DictionaryInner)` allow for creating a tokenizer directly from a dynamically built dictionary instance, enhancing flexibility for testing and applications that generate dictionaries on-the-fly.

* **Owned Token Type (`TokenBuf`)**  
  A new owned token type, `TokenBuf`, has been introduced alongside the existing borrowed `Token<'a>`. Following the familiar `Path`/`PathBuf` pattern in Rust's standard library. This makes it easy to store tokenization results, modify them, or send them across threads without lifetime complications.

* **Built-in Dictionary Downloader and Manager**  
  Getting started is now easier than ever. You can download and set up pre-compiled preset dictionaries (e.g., IPADIC, UNIDIC) with a single function call.
  * `Dictionary::from_preset_with_download()`: Handles downloading, checksum verification, and caching automatically.
  * `Dictionary::from_zstd()`: Intelligently manages `zstd`-compressed dictionaries by decompressing them to a local cache on the first run. It also automatically detects and converts legacy `bincode`-formatted dictionaries (when the legacy feature is enabled), caching them in the modern format in the background for future fast loads.

## Features

### Fast tokenization

Vibrato is a Rust reimplementation of the fast tokenizer [MeCab](https://taku910.github.io/mecab/),
although its implementation has been simplified and optimized for even faster tokenization.
Especially for language resources with a large matrix
(e.g., [`unidic-cwj-3.1.1`](https://clrd.ninjal.ac.jp/unidic/back_number.html#unidic_cwj) with a matrix of 459 MiB),
Vibrato will run faster thanks to cache-efficient id mappings.

For example, the following figure shows an experimental result of
tokenization time with MeCab and its reimplementations.
The detailed experimental settings and other results are available on [Wiki](https://github.com/daac-tools/vibrato/wiki/Speed-Comparison).

![](./figures/comparison.svg)

### MeCab compatibility

Vibrato supports options for outputting tokenized results identical to MeCab, such as ignoring whitespace.

### Training parameters

Vibrato also supports training parameters (or costs) in dictionaries from your corpora.
The detailed description can be found [here](./docs/train.md).

## Basic usage

This software is implemented in Rust.
First of all, install `rustc` and `cargo` following the [official instructions](https://www.rust-lang.org/tools/install).


### As a Rust Library (Recommended)

The easiest way to get started is by using `vibrato-rkyv` as a library and downloading a pre-compiled preset dictionary.

**1. Add `vibrato-rkyv` to your dependencies**

Add the following to your `Cargo.toml`. The dictionary download feature is enabled by default.

```toml
[dependencies]
vibrato-rkyv = "x.y.z"
```

**2. Download a dictionary and tokenize text**

The `Dictionary::from_preset_with_download()` function handles everything: downloading, verifying the checksum, and caching the dictionary in a specified directory for future runs.

```rust
use std::path::PathBuf;
use vibrato_rkyv::{dictionary::PresetDictionaryKind, Dictionary, Tokenizer};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Specify a directory to cache the dictionary.
    let mut cache_dir = dirs::cache_dir().unwrap_or_else(|| PathBuf::from(".cache"));
    cache_dir.push("vibrato-rkyv");
    std::fs::create_dir_all(&cache_dir)?;

    // Downloads and loads a preset dictionary (e.g., IPADIC).
    // The dictionary is cached in the specified directory, so subsequent runs are instantaneous.
    let dict = Dictionary::from_preset_with_download(
        PresetDictionaryKind::Ipadic,
        &cache_dir,
    )?;

    // Create a tokenizer with the loaded dictionary.
    let tokenizer = Tokenizer::new(dict);

    // A worker holds internal states for tokenization and can be reused.
    let mut worker = tokenizer.new_worker();

    worker.set_text("本とカレーの街神保町へようこそ。");
    worker.tokenize();

    // Iterate over tokens.
    for token in worker.token_iter() {
        println!("{}\t{}", token.surface(), token.feature());
    }

    Ok(())
}
```

### As a Command-Line Tool

**1. Prepare a Dictionary**

You need a dictionary file (`.dic`) compatible with `vibrato-rkyv`. Use the `compiler` tool to build a dictionary from your source CSV files.

```bash
# Example of compiling a dictionary
$ cargo run --release -p compiler -- build \
    --lexicon-in path/to/lex.csv \
    --matrix-in path/to/matrix.def \
    --char-in path/to/char.def \
    --unk-in path/to/unk.def \
    --sysdic-out system.dic
```

**2. Tokenize Sentences**

Pipe your text to the `tokenize` command and specify the dictionary path with `-i`.

```bash
$ echo '本とカレーの街神保町へようこそ。' | cargo run --release -p tokenize -- -i path/to/system.dic
```

The result will be printed in MeCab format. To output tokens separated by spaces, use the `-O wakati` option.

```
本	名詞,一般,*,*,*,*,本,ホン,ホン
と	助詞,並立助詞,*,*,*,*,と,ト,ト
カレー	名詞,固有名詞,地域,一般,*,*,カレー,カレー,カレー
の	助詞,連体化,*,*,*,*,の,ノ,ノ
街	名詞,一般,*,*,*,*,街,マチ,マチ
神保	名詞,固有名詞,地域,一般,*,*,神保,ジンボウ,ジンボー
町	名詞,接尾,地域,*,*,*,町,マチ,マチ
へ	助詞,格助詞,一般,*,*,*,へ,ヘ,エ
ようこそ	感動詞,*,*,*,*,*,ようこそ,ヨウコソ,ヨーコソ
。	記号,句点,*,*,*,*,。,。,。
EOS
```

## Advanced Usage

### MeCab-compatible Options

Vibrato is a reimplementation of the MeCab algorithm, but its default tokenization results may differ. For example, Vibrato treats spaces as tokens by default, whereas MeCab ignores them.

To get results identical to MeCab, use the `-S` (ignore spaces) and `-M` (maximum unknown word length) flags.

```bash
# Get MeCab-compatible output
$ echo 'mens second bag' | cargo run --release -p tokenize -- -i path/to/system.dic -S -M 24
mens	名詞,固有名詞,組織,*,*,*,*
second	名詞,一般,*,*,*,*,*
bag	名詞,固有名詞,組織,*,*,*,*
EOS
```
*Note: In rare cases, results may still differ due to tie-breaking in cost calculation.*

### Using a User Dictionary

**IMPORTANT:** In `vibrato-rkyv`, user dictionaries can no longer be specified as a runtime option. They must be compiled into the system dictionary beforehand.

**Option: With the `compiler full-build` command**

If you are training a new dictionary, the `full-build` command is the recommended way to include a user dictionary. It handles the entire pipeline: training, generating source files (including the user lexicon), and building the final binary. Use the `--user-lexicon-in` option.

```bash
$ cargo run --release -p compiler -- full-build \
    -t path/to/corpus.txt \
    -l path/to/seed_lex.csv \
    --user-lexicon-in path/to/my_user_dic.csv \
    ... # other required arguments
    -o ./my_dictionary
```

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## References

Technical details of Vibrato are available in the following resources:

- 神田峻介, 赤部晃一, 後藤啓介, 小田悠介.
  [最小コスト法に基づく形態素解析におけるCPUキャッシュの効率化](https://www.anlp.jp/proceedings/annual_meeting/2023/pdf_dir/C2-4.pdf),
  言語処理学会第29回年次大会 (NLP2023).
- 赤部晃一, 神田峻介, 小田悠介.
  [CRFに基づく形態素解析器のスコア計算の分割によるモデルサイズと解析速度の調整](https://www.anlp.jp/proceedings/annual_meeting/2023/pdf_dir/C2-1.pdf),
  言語処理学会第29回年次大会 (NLP2023).
- [MeCab互換な形態素解析器Vibratoの高速化技法](https://tech.legalforce.co.jp/entry/2022/09/20/133132),
  LegalOn Technologies Engineering Blog (2022-09-20).
