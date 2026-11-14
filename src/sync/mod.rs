//! Kindle Device Synchronization Engine
//!
//! Handles safe transfer of KFX books, lockscreen cover thumbnails, and Word Wise (.kll) sidecars
//! to Amazon Kindle devices over both UMS (USB Mass Storage) and Linux GVFS MTP (`gio copy`).

pub mod catalog;
pub mod device;
pub mod sanitize;
pub mod transfer;
pub mod types;

pub use catalog::*;
pub use device::*;
pub use sanitize::*;
pub use transfer::*;
pub use types::*;

use std::path::{Path, PathBuf};

/// Global mutual exclusion lock ensuring all write and deletion operations targeting
/// Kindle storage (VFAT FAT table allocations, companion .sdr writes, and MTP commands)
/// are strictly serialized to prevent filesystem corruption, USB FIFO stalls, and MTP deadlocks.
pub(crate) static DEVICE_SYNC_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl KindleSync {
    /// Sanitizes an e-book title to be completely safe for FAT32 / VFAT storage
    /// and prevents directory traversal attacks (CWE-22 / CWE-73).
    #[inline]
    pub fn sanitize_filename(name: &str) -> String {
        sanitize::sanitize_filename(name)
    }

    /// Sanitizes an ASIN identifier against path traversal and special characters.
    #[inline]
    pub fn sanitize_asin(asin: &str) -> String {
        sanitize::sanitize_asin(asin)
    }

    /// Queries filesystem storage statistics (used_gb, total_gb).
    #[inline]
    pub fn get_storage_stats(path: &Path) -> Option<(f32, f32)> {
        device::get_storage_stats(path)
    }

    /// Accurately resolves the specific Kindle hardware model from mount hint or on-device files.
    #[inline]
    pub fn resolve_model_name(root: &Path, hint: &str) -> String {
        device::resolve_model_name(root, hint)
    }

    /// On Linux, inspects `/sys/bus/usb/devices/` to detect any connected Amazon Kindle USB devices
    /// (Vendor ID 0x1949 / Lab126) that are currently not mounted as a filesystem.
    #[inline]
    pub fn probe_unmounted_usb_kindle() -> Option<UnmountedKindleUsb> {
        device::probe_unmounted_usb_kindle()
    }

    /// Parameterized helper to probe an arbitrary sysfs USB devices directory.
    #[inline]
    pub fn probe_unmounted_usb_kindle_in(usb_dir: &Path) -> Option<UnmountedKindleUsb> {
        device::probe_unmounted_usb_kindle_in(usb_dir)
    }

    /// Detects connected Kindle root directory and metadata (GVFS MTP or USB UMS mount).
    #[inline]
    pub fn detect_device_info() -> Option<KindleDeviceInfo> {
        device::detect_device_info()
    }

    /// Detects connected Kindle root directory (GVFS MTP or USB UMS mount).
    #[inline]
    pub fn detect_kindle_root() -> Option<PathBuf> {
        device::detect_kindle_root()
    }

    /// Safely flushes dirty filesystem caches and unmounts the connected Kindle device.
    #[inline]
    pub fn eject_device(root: &Path) -> Result<(), SyncError> {
        device::eject_device(root)
    }

    /// Scans the Kindle documents directory (including author subfolders up to max_depth 3) for valid e-book files.
    #[inline]
    pub fn scan_device_files(doc_path: &Path) -> Vec<KindleBookFile> {
        catalog::scan_device_files(doc_path)
    }

    /// Deletes an e-book file and its companion `.sdr` directory from the device safely.
    #[inline]
    pub fn delete_book_file_safe(allowed_root: Option<&Path>, full_path: &Path) -> Result<(), std::io::Error> {
        catalog::delete_book_file_safe(allowed_root, full_path)
    }

    /// Deletes an e-book file and its companion `.sdr` directory from the device.
    #[inline]
    pub fn delete_book_file(full_path: &Path) -> Result<(), std::io::Error> {
        catalog::delete_book_file(full_path)
    }

    /// Synchronizes a converted KFX file, multi-identifier cover thumbnails, and optional sidecars to the Kindle.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn sync_to_device(
        device_root: &Path,
        kfx_source: &Path,
        book_title: &str,
        asin: &str,
        content_id: Option<&str>,
        cde_content_type: Option<&str>,
        book_language: Option<&str>,
        cover_jpeg: Option<&[u8]>,
        kll_bytes: Option<&[u8]>,
    ) -> Result<PathBuf, SyncError> {
        transfer::sync_to_device(
            device_root,
            kfx_source,
            book_title,
            asin,
            content_id,
            cde_content_type,
            book_language,
            cover_jpeg,
            kll_bytes,
        )
    }

    /// Purges .bad_file markers, incompatible serialized state files, and orphaned enhancement sidecars.
    #[inline]
    pub fn clean_stale_sdr_files(
        sdr_dir: &Path,
        current_asin: Option<&str>,
        purge_word_wise: bool,
    ) {
        catalog::clean_stale_sdr_files(sdr_dir, current_asin, purge_word_wise)
    }

    /// Clears the read-only attribute on an existing destination file so that atomic overwrites or renames succeed.
    #[inline]
    #[allow(clippy::permissions_set_readonly_false)]
    pub fn prepare_destination_for_overwrite(dst: &Path) {
        transfer::prepare_destination_for_overwrite(dst)
    }

    #[inline]
    #[allow(dead_code)]
    pub fn copy_file(src: &Path, dst: &Path) -> Result<(), SyncError> {
        transfer::copy_file(src, dst)
    }

    #[inline]
    #[allow(dead_code)]
    pub fn execute_gio_copy_with_timeout(
        src: &Path,
        dst: &Path,
        timeout: std::time::Duration,
    ) -> Result<(), SyncError> {
        transfer::execute_gio_copy_with_timeout(src, dst, timeout)
    }

    #[inline]
    #[allow(dead_code)]
    pub fn write_bytes(dst: &Path, data: &[u8]) -> Result<(), SyncError> {
        transfer::write_bytes(dst, data)
    }
}
