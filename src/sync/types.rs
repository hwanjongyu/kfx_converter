use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SyncError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Device not connected or documents directory not found at {0:?}")]
    DeviceNotFound(PathBuf),
    #[error("gio command failed: {0}")]
    GioFailed(String),
    #[error("Security violation: {0}")]
    SecurityViolation(String),
}

/// Information about a detected Kindle device
#[derive(Debug, Clone)]
pub struct KindleDeviceInfo {
    pub root: PathBuf,
    pub documents_dir: PathBuf,
    pub model_name: String,
    pub used_gb: f32,
    pub total_gb: f32,
}

/// Represents an unmounted Amazon Kindle hardware device detected via USB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmountedKindleUsb {
    pub vendor_id: String,
    pub product_id: String,
    pub product_name: String,
}

/// Represents an e-book file stored on a Kindle device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindleBookFile {
    pub name: String,
    pub title: String,
    pub author: String,
    pub format: String,
    pub size_formatted: String,
    pub size_bytes: u64,
    pub modified: String,
    pub modified_timestamp: u64,
    pub full_path: PathBuf,
    pub has_word_wise: bool,
    pub has_sdr: bool,
}

/// Marker facade struct hosting static synchronization engine methods.
pub struct KindleSync;
