# 🎤 vibrato-rkyv: VIterbi-Based acceleRAted TOkenizer with rkyv

**Note:** This is a fork of the original [daac-tools/vibrato](https://github.com/daac-tools/vibrato) modified to use the `rkyv` serialization framework for significantly faster dictionary loading.

[![Crates.io](https://img.shields.io/crates/v/vibrato-rkyv)](https://crates.io/crates/vibrato-rkyv)
[![Documentation](https://docs.rs/vibrato-rkyv/badge.svg)](https://docs.rs/vibrato-rkyv)
[![Build Status](https://github.com/stellanomia/vibrato-rkyv/actions/workflows/rust.yml/badge.svg)](https://github.com/stellanomia/vibrato-rkyv/actions)
[![Build Status](https://github.com/daac-tools/vibrato/actions/workflows/rust.yml/badge.svg)](https://github.com/daac-tools/vibrato/actions)

Vibrato is a fast implementation of tokenization (or morphological analysis) based on the Viterbi algorithm.

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

### 1. Dictionary preparation

You can easily get started by downloading a precompiled dictionary compatible with this version.

You must compile system dictionaries from raw resources using the `compile` command included in this repository. Dictionaries compiled with the original `vibrato` are **not compatible**.

```
# Example of compiling a dictionary
$ cargo run --release -p compile -- -i path/to/lex.csv ... -o system.dic
```

### 2. Tokenization

To tokenize sentences using the system dictionary, run the following command. The dictionary is loaded from the file path.

```
$ echo '本とカレーの街神保町へようこそ。' | cargo run --release -p tokenize -- -i path/to/system.dic
```

The resultant tokens will be output in the Mecab format.

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

If you want to output tokens separated by spaces, specify `-O wakati`.

```
$ echo '本とカレーの街神保町へようこそ。' | cargo run --release -p tokenize -- -i ipadic-mecab-2_7_0/system.dic.zst -O wakati
本 と カレー の 街 神保 町 へ ようこそ 。
```

### Notes for Vibrato APIs
This version of Vibrato is optimized for loading dictionaries from a file path using memory-mapping.

```rust
use vibrato_rkyv::Dictionary;

// Recommended: Load from path for zero-copy deserialization
let dict = Dictionary::from_path("path/to/system.dic")?;
```

If you need to load from a reader (e.g., a compressed stream), all data will be loaded into memory.

```rust
use std::fs::File;
use vibrato_rkyv::Dictionary;

// Requires zstd crate crate
let reader = zstd::Decoder::new(File::open("path/to/system.dic.zst")?)?;
let dict = Dictionary::read(reader)?;
```

## Tokenization options

### MeCab-compatible options

Vibrato is a reimplementation of the MeCab algorithm,
but with the default settings it can produce different tokens from MeCab.

For example, MeCab ignores spaces (more precisely, `SPACE` defined in `char.def`) in tokenization.

```
$ echo "mens second bag" | mecab
mens	名詞,固有名詞,組織,*,*,*,*
second	名詞,一般,*,*,*,*,*
bag	名詞,固有名詞,組織,*,*,*,*
EOS
```

However, Vibrato handles such spaces as tokens with the default settings.

```
$ echo 'mens second bag' | cargo run --release -p tokenize -- -i ipadic-mecab-2_7_0/system.dic.zst
mens	名詞,固有名詞,組織,*,*,*,*
 	記号,空白,*,*,*,*,*
second	名詞,固有名詞,組織,*,*,*,*
 	記号,空白,*,*,*,*,*
bag	名詞,固有名詞,組織,*,*,*,*
EOS
```

If you want to obtain the same results as MeCab, specify the arguments `-S` and `-M 24`.

```
$ echo 'mens second bag' | cargo run --release -p tokenize -- -i ipadic-mecab-2_7_0/system.dic.zst -S -M 24
mens	名詞,固有名詞,組織,*,*,*,*
second	名詞,一般,*,*,*,*,*
bag	名詞,固有名詞,組織,*,*,*,*
EOS
```

`-S` indicates if spaces are ignored.
`-M` indicates the maximum grouping length for unknown words.

#### Notes

There are corner cases where tokenization results in different outcomes due to cost tiebreakers.
However, this would be not an essential problem.

### User dictionary

**IMPORTANT:** In this `rkyv`-based version, the user dictionary is **no longer a command-line option** for the `tokenize` command.

Due to the immutable, zero-copy nature of the dictionary, user dictionaries must be **compiled into the system dictionary beforehand**.

To use a user dictionary, you need to create a `DictionaryInner` object that includes the user dictionary and then serialize it. The `compile` command or a custom build script can be used for this purpose.

For example, you can create a combined dictionary using a build script like this:

```rust
// A simplified example of a build script

use std::fs::File;
use vibrato_rkyv::{SystemDictionaryBuilder, Dictionary};

// 1. Build a DictionaryInner from the system dictionary components
let dict_inner = SystemDictionaryBuilder::from_readers(...)?.reset_user_lexicon_from_reader(
    Some(File::open("user.csv")?)
)?;

// 2. Write the combined DictionaryInner to a file
let mut file = File::create("system_with_user.dic")?;
dict_inner.write(&mut file)?;
```

Then, use the generated `system_with_user.dic` with the `tokenize` command.

```
$ echo '本とカレーの街神保町へようこそ。' | cargo run --release -p tokenize -- -i system_with_user.dic
本とカレーの街	カスタム名詞,ホントカレーノマチ
神保町	カスタム名詞,ジンボチョウ
へ	助詞,格助詞,一般,*,*,*,へ,ヘ,エ
ようこそ	感動詞,ヨーコソ,Welcome,欢迎欢迎,Benvenuto,Willkommen
。	記号,句点,*,*,*,*,。,。,。
EOS
```
## More advanced usages

The directory [docs](./docs/) provides descriptions of more advanced usages such as training or benchmarking.

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
