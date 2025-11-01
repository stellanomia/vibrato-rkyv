mod dual_connector;
mod matrix_connector;
mod raw_connector;

use bincode::{Decode, Encode};

pub use crate::legacy::dictionary::connector::dual_connector::DualConnector;
pub use crate::legacy::dictionary::connector::matrix_connector::MatrixConnector;
pub use crate::legacy::dictionary::connector::raw_connector::RawConnector;

#[derive(Decode, Encode)]
pub enum ConnectorWrapper {
    Matrix(MatrixConnector),
    Raw(RawConnector),
    Dual(DualConnector),
}
