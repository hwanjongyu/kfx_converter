//! Packager error definitions.

use thiserror::Error;
use crate::container::ContainerError;

#[derive(Error, Debug)]
pub enum PackagerError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Container packing error: {0}")]
    Container(#[from] ContainerError),
    #[error("Pre-flight reference validation error: {0}")]
    Validation(#[from] crate::validator::ValidationError),
}
