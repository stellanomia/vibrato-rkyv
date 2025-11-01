use bincode::{Decode, Encode};

/// Mapper for connection ids.
#[derive(Decode, Encode)]
pub struct ConnIdMapper {
    left: Vec<u16>,
    right: Vec<u16>,
}
