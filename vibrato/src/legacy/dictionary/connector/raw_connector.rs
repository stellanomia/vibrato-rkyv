pub mod scorer;


use bincode::{Decode, Encode};

use crate::legacy::dictionary::connector::raw_connector::scorer::{
    Scorer, U31x8,
};

#[derive(Decode, Encode)]
pub struct RawConnector {
    right_feat_ids: Vec<U31x8>,
    left_feat_ids: Vec<U31x8>,
    feat_template_size: usize,
    scorer: Scorer,
}
