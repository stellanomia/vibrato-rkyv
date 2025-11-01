use bincode::{Decode, Encode};

use crate::legacy::dictionary::connector::raw_connector::scorer::{
    Scorer, U31x8,
};
use crate::legacy::dictionary::connector::MatrixConnector;

#[derive(Decode, Encode)]
pub struct DualConnector {
    matrix_connector: MatrixConnector,
    right_conn_id_map: Vec<u16>,
    left_conn_id_map: Vec<u16>,
    right_feat_ids: Vec<U31x8>,
    left_feat_ids: Vec<U31x8>,
    raw_scorer: Scorer,
}
