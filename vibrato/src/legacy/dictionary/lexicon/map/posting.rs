use bincode::{Decode, Encode};

#[derive(Decode, Encode)]
pub struct Postings {
    // Sets of ids are stored by interleaving their length and values.
    // Then, 8 bits would be sufficient to represent the length in most cases, and
    // serializing `data` into a byte sequence can reduce the memory usage.
    // However, the memory usage is slight compared to that of the connection matrix.
    // Thus, we implement `data` as `Vec<u32>` for simplicity.
    data: Vec<u32>,
}
