use bincode::{Decode, Encode};

/// Matrix of connection costs.
#[derive(Decode, Encode)]
pub struct MatrixConnector {
    data: Vec<i16>,
    num_right: usize,
    num_left: usize,
}
