use bincode::{Decode, Encode};

#[derive(Default, Decode, Encode)]
pub struct WordFeatures {
    features: Vec<String>,
}
