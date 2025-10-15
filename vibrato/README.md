# vibrato-rkyv

Vibrato is a fast implementation of tokenization (or morphological analysis) based on the Viterbi algorithm.

## API documentation

https://docs.rs/vibrato-rkyv

## Performance

`vibrato-rkyv` provides two extremely fast ways to load dictionaries:

- `Dictionary::from_path()`  
  Loads an already-decompressed dictionary file (e.g. `*.dic`) directly.  
  Typical load time is around 20–30 µs, effectively instant.

- `Dictionary::from_zstd()`  
  Transparently loads a Zstandard-compressed dictionary (e.g. `*.dic.zst`).  
  On the first run, it decompresses the file and caches it under `decompressed/` next to the original.  
  Subsequent runs reuse the cached file, achieving the same speed as `from_path`.  
  The cache is automatically invalidated if the original compressed file changes.

| Method | Description | Typical load time |
|---------|--------------|------------------|
| `from_path` | Directly loads a decompressed dictionary | 10~1000 µs (approximately) |
| `from_zstd` | Loads a compressed dictionary (cached) | 1st run ≈ .. s → cached ≈ 10~1000 µs (approximately) |

This design enables near-instant startup even with large Unidic dictionaries.

## License

Licensed under either of

 * Apache License, Version 2.0  
   ([LICENSE-APACHE](../LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license  
   ([LICENSE-MIT](../LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.