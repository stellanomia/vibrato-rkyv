mod dual_connector;
mod matrix_connector;
mod raw_connector;

use rkyv::{Archive, Deserialize, Serialize};

pub use crate::dictionary::connector::dual_connector::DualConnector;
pub use crate::dictionary::connector::matrix_connector::MatrixConnector;
pub use crate::dictionary::connector::raw_connector::RawConnector;
use crate::dictionary::mapper::ConnIdMapper;

pub trait ConnectorView {
    /// Returns maximum number of left connection ID
    fn num_left(&self) -> usize;

    /// Returns maximum number of right connection ID
    fn num_right(&self) -> usize;
}

pub trait Connector: ConnectorView {
    /// Do NOT make this function public to maintain consistency in
    /// the connection-id mapping among members of `Dictionary`.
    /// The consistency is managed in `Dictionary`.
    fn map_connection_ids(&mut self, mapper: &ConnIdMapper);
}

pub trait ConnectorCost: ConnectorView {
    /// Gets the value of the connection matrix
    fn cost(&self, right_id: u16, left_id: u16) -> i32;
}

#[derive(Archive, Serialize, Deserialize)]
pub enum ConnectorWrapper {
    Matrix(MatrixConnector),
    Raw(RawConnector),
    Dual(DualConnector),
}

impl ConnectorView for ConnectorWrapper {
    fn num_left(&self) -> usize {
        match self {
            Self::Matrix(c) => c.num_left(),
            Self::Raw(c) => c.num_left(),
            Self::Dual(c) => c.num_left(),
        }
    }
    fn num_right(&self) -> usize {
        match self {
            Self::Matrix(c) => c.num_right(),
            Self::Raw(c) => c.num_right(),
            Self::Dual(c) => c.num_right(),
        }
    }
}

impl Connector for ConnectorWrapper {
    fn map_connection_ids(&mut self, mapper: &ConnIdMapper) {
        match self {
            Self::Matrix(c) => c.map_connection_ids(mapper),
            Self::Raw(c) => c.map_connection_ids(mapper),
            Self::Dual(c) => c.map_connection_ids(mapper),
        }
    }
}

impl ConnectorView for ArchivedConnectorWrapper {
    fn num_left(&self) -> usize {
        match self {
            Self::Matrix(c) => c.num_left(),
            Self::Raw(c) => c.num_left(),
            Self::Dual(c) => c.num_left(),
        }
    }
    fn num_right(&self) -> usize {
        match self {
            Self::Matrix(c) => c.num_right(),
            Self::Raw(c) => c.num_right(),
            Self::Dual(c) => c.num_right(),
        }
    }
}
