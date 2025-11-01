use bincode::{
    de::{BorrowDecode, BorrowDecoder, Decoder},
    enc::Encoder,
    error::{DecodeError, EncodeError},
    Decode, Encode,
};

use crate::legacy::errors::Result;

pub struct Trie {
    da: crawdad::Trie,
}

impl Encode for Trie {
    fn encode<E: Encoder>(&self, encoder: &mut E) -> Result<(), EncodeError> {
        Encode::encode(&self.da.serialize_to_vec(), encoder)?;
        Ok(())
    }
}

impl<Context> Decode<Context> for Trie {
    fn decode<D: Decoder>(decoder: &mut D) -> Result<Self, DecodeError> {
        let data: Vec<u8> = Decode::decode(decoder)?;
        let (da, _) = crawdad::Trie::deserialize_from_slice(&data);
        Ok(Self { da })
    }
}

impl<'de, Context> BorrowDecode<'de, Context> for Trie {
    fn borrow_decode<D: BorrowDecoder<'de>>(decoder: &mut D) -> Result<Self, DecodeError> {
        let data: &[u8] = BorrowDecode::borrow_decode(decoder)?;
        let (da, _) = crawdad::Trie::deserialize_from_slice(data);
        Ok(Self { da })
    }
}
