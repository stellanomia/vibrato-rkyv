//! Definition of errors.

use std::error::Error;
use std::fmt;

/// A specialized Result type for Vibrato.
pub type Result<T, E = VibratoError> = std::result::Result<T, E>;

/// The error type for Vibrato.
#[derive(Debug, thiserror::Error)]
pub enum VibratoError {
    /// The error variant for [`InvalidArgumentError`].
    InvalidArgument(InvalidArgumentError),

    /// The error variant for [`InvalidFormatError`].
    InvalidFormat(InvalidFormatError),

    /// The error variant for [`InvalidStateError`].
    InvalidState(InvalidStateError),

    /// The error variant for [`TryFromIntError`](std::num::TryFromIntError).
    TryFromInt(std::num::TryFromIntError),

    /// The error variant for [`ParseFloatError`](std::num::ParseFloatError).
    ParseFloat(std::num::ParseFloatError),

    /// The error variant for [`ParseIntError`](std::num::ParseIntError).
    ParseInt(std::num::ParseIntError),

    /// The error variant for [`std::io::Error`].
    StdIo(std::io::Error),

    /// The error variant for [`std::str::Utf8Error`].
    Utf8(std::str::Utf8Error),

    /// The error variant for [`RucrfError`](rucrf_rkyv::errors::RucrfError).
    #[cfg(feature = "train")]
    Crf(rucrf_rkyv::errors::RucrfError),

    /// The error variant for [`std::io::Error`](std::io::Error).
    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[error(transparent)]
    /// The error variant for [`rkyv::rancor::Error`](rkyv::rancor::Error).
    RkyvError(#[from] rkyv::rancor::Error),
}

impl VibratoError {
    pub(crate) fn invalid_argument<S>(arg: &'static str, msg: S) -> Self
    where
        S: Into<String>,
    {
        Self::InvalidArgument(InvalidArgumentError {
            arg,
            msg: msg.into(),
        })
    }

    pub(crate) fn invalid_format<S>(arg: &'static str, msg: S) -> Self
    where
        S: Into<String>,
    {
        Self::InvalidFormat(InvalidFormatError {
            arg,
            msg: msg.into(),
        })
    }

    pub(crate) fn invalid_state<S, M>(msg: S, cause: M) -> Self
    where
        S: Into<String>,
        M: Into<String>,
    {
        Self::InvalidState(InvalidStateError {
            msg: msg.into(),
            cause: cause.into(),
        })
    }
}

impl fmt::Display for VibratoError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidArgument(e) => e.fmt(f),
            Self::InvalidFormat(e) => e.fmt(f),
            Self::InvalidState(e) => e.fmt(f),
            Self::TryFromInt(e) => e.fmt(f),
            Self::ParseFloat(e) => e.fmt(f),
            Self::ParseInt(e) => e.fmt(f),
            Self::StdIo(e) => e.fmt(f),
            Self::Utf8(e) => e.fmt(f),
            Self::IoError(e) => e.fmt(f),
            Self::RkyvError(e) => e.fmt(f),

            #[cfg(feature = "train")]
            Self::Crf(e) => e.fmt(f),
        }
    }
}

/// Error used when the argument is invalid.
#[derive(Debug)]
pub struct InvalidArgumentError {
    /// Name of the argument.
    pub(crate) arg: &'static str,

    /// Error message.
    pub(crate) msg: String,
}

impl fmt::Display for InvalidArgumentError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "InvalidArgumentError: {}: {}", self.arg, self.msg)
    }
}

impl Error for InvalidArgumentError {}

/// Error used when the input format is invalid.
#[derive(Debug)]
pub struct InvalidFormatError {
    /// Name of the format.
    pub(crate) arg: &'static str,

    /// Error message.
    pub(crate) msg: String,
}

impl fmt::Display for InvalidFormatError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "InvalidFormatError: {}: {}", self.arg, self.msg)
    }
}

impl Error for InvalidFormatError {}

/// Error used when the state is invalid.
#[derive(Debug)]
pub struct InvalidStateError {
    /// Error message.
    pub(crate) msg: String,

    /// Underlying cause of the error.
    pub(crate) cause: String,
}

impl fmt::Display for InvalidStateError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "InvalidStateError: {}: {}", self.msg, self.cause)
    }
}

impl Error for InvalidStateError {}


impl From<std::num::TryFromIntError> for VibratoError {
    fn from(error: std::num::TryFromIntError) -> Self {
        Self::TryFromInt(error)
    }
}

impl From<std::num::ParseFloatError> for VibratoError {
    fn from(error: std::num::ParseFloatError) -> Self {
        Self::ParseFloat(error)
    }
}

impl From<std::num::ParseIntError> for VibratoError {
    fn from(error: std::num::ParseIntError) -> Self {
        Self::ParseInt(error)
    }
}

impl From<std::str::Utf8Error> for VibratoError {
    fn from(error: std::str::Utf8Error) -> Self {
        Self::Utf8(error)
    }
}

#[cfg(feature = "train")]
impl From<rucrf_rkyv::errors::RucrfError> for VibratoError {
    fn from(error: rucrf_rkyv::errors::RucrfError) -> Self {
        Self::Crf(error)
    }
}
