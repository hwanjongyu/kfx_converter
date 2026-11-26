//! State Management & Kindle Hardware Discovery for GPUI

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StudioTab {
    DualPane,
    Convert,
    LocalFolder,
    Explorer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConverterMode {
    DropQueue,
    FolderBrowser,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalEpubStatus {
    Unconverted,
    InQueue,
    Processing { stage: String, progress: u8 },
    ConvertedLocal,
    OnDevice,
    Error(String),
}

impl LocalEpubStatus {
    pub fn priority(&self) -> u8 {
        match self {
            LocalEpubStatus::Unconverted => 0,
            LocalEpubStatus::InQueue => 1,
            LocalEpubStatus::Processing { .. } => 2,
            LocalEpubStatus::ConvertedLocal => 3,
            LocalEpubStatus::OnDevice => 4,
            LocalEpubStatus::Error(_) => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EpubSortField {
    #[default]
    Title,
    Author,
    DateModified,
    FileSize,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

impl SortDirection {
    pub fn toggle(&self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Self::Ascending => "▲",
            Self::Descending => "▼",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BatchProgressState {
    pub total_tasks: usize,
    pub completed_tasks: usize,
    pub failed_tasks: usize,
    pub active_tasks: usize,
    pub current_filename: String,
    pub current_stage: String,
    pub is_sync: bool,
    pub is_cancelled: bool,
}

impl BatchProgressState {
    pub fn overall_progress(&self) -> f32 {
        if self.total_tasks == 0 {
            0.0
        } else {
            let finished = self
                .completed_tasks
                .saturating_add(self.failed_tasks)
                .min(self.total_tasks);
            (finished as f32 / self.total_tasks as f32).clamp(0.0, 1.0)
        }
    }

    pub fn overall_percentage(&self) -> u8 {
        (self.overall_progress() * 100.0).round() as u8
    }
}

/// Outcome of a single conversion task used to synchronize tasks, local folder items,
/// batch progress counters, and Kindle device state through a unified path.
#[derive(Debug, Clone)]
pub enum ConversionOutcome {
    Finished(ConversionResult),
    Failed(String),
    Cancelled,
}

/// Formats conversion or synchronization error messages cleanly with context,
/// collapsing embedded newlines and clamping on UTF-8 char boundaries without panicking.
pub fn format_task_error(context: &str, err: &str) -> String {
    const MAX_ERROR_CHARS: usize = 160;
    let normalized: String = err.split_whitespace().collect::<Vec<_>>().join(" ");
    let body = if normalized.is_empty() {
        "Unknown error".to_string()
    } else {
        normalized
    };

    let prefix = context.trim();
    let combined = if prefix.is_empty()
        || body
            .get(..prefix.len() + 1)
            .is_some_and(|head| head.eq_ignore_ascii_case(&format!("{prefix}:")))
    {
        body
    } else {
        format!("{prefix}: {body}")
    };

    if combined.chars().count() <= MAX_ERROR_CHARS {
        combined
    } else {
        let mut out: String = combined.chars().take(MAX_ERROR_CHARS - 3).collect();
        out.push_str("...");
        out
    }
}

/// Resolves and validates an output folder target for launching the OS file manager.
///
/// Accepts either an existing directory or an existing file (resolving to its parent directory),
/// canonicalizes the directory, strips Windows verbatim `\\?\` prefixes where safe, and ensures
/// the resolved path never begins with `-`.
pub fn resolve_output_folder_target(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("Output path is empty.".to_string());
    }

    let dir = if path.is_file() {
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| "Output file has no parent folder.".to_string())?
    } else if path.is_dir() {
        path
    } else {
        return Err(format!("Output folder does not exist: {}", path.display()));
    };

    if !dir.is_dir() {
        return Err(format!("Target is not a directory: {}", dir.display()));
    }

    let canon = std::fs::canonicalize(dir)
        .map_err(|e| format!("Failed to resolve output folder '{}': {}", dir.display(), e))?;

    if !canon.is_dir() {
        return Err(format!(
            "Resolved target is not a directory: {}",
            canon.display()
        ));
    }

    #[cfg(windows)]
    let resolved = {
        let raw = canon.to_string_lossy();
        if let Some(stripped) = raw.strip_prefix(r"\\?\") {
            if !stripped.starts_with("UNC\\") {
                PathBuf::from(stripped)
            } else {
                canon
            }
        } else {
            canon
        }
    };
    #[cfg(not(windows))]
    let resolved = canon;

    if resolved.to_string_lossy().starts_with('-') {
        return Err("Refusing to open folder path starting with '-'.".to_string());
    }

    Ok(resolved)
}

/// Collects lowercase `.kfx` file stems present in `doc_dir` (including author subfolders
/// up to depth 3) via `KindleSync::scan_device_files`, acquiring `DEVICE_SYNC_LOCK` and
/// skipping symlinks, hidden directories, and `.sdr` sidecars.
pub fn collect_device_kfx_stems(doc_dir: &Path) -> std::collections::HashSet<String> {
    kfx_converter::KindleSync::scan_device_files(doc_dir)
        .into_iter()
        .filter(|f| f.format.eq_ignore_ascii_case("KFX"))
        .filter_map(|f| {
            f.full_path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|stem| stem.to_lowercase())
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct LocalEpubItem {
    pub id: u64,
    pub path: PathBuf,
    pub filename: String,
    pub title: String,
    pub author: String,
    pub file_size_bytes: u64,
    pub file_size_str: String,
    pub modified_timestamp: u64,
    pub modified_str: String,
    pub is_selected: bool,
    pub status: LocalEpubStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanPhase {
    Discovering {
        found_files: usize,
    },
    ReadingMetadata {
        processed: usize,
        total: usize,
        current_file: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanProgressState {
    pub phase: ScanPhase,
}

impl ScanProgressState {
    pub fn discovering() -> Self {
        Self {
            phase: ScanPhase::Discovering { found_files: 0 },
        }
    }

    pub fn fraction(&self) -> f32 {
        match &self.phase {
            ScanPhase::Discovering { .. } => 0.0,
            ScanPhase::ReadingMetadata {
                processed, total, ..
            } => {
                if *total == 0 {
                    1.0
                } else {
                    (*processed as f32 / *total as f32).clamp(0.0, 1.0)
                }
            }
        }
    }

    pub fn percentage(&self) -> u8 {
        (self.fraction() * 100.0).round() as u8
    }
}

#[derive(Debug, Clone)]
pub struct LocalFolderState {
    pub current_folder: Option<PathBuf>,
    pub recursive_scan: bool,
    pub items: Vec<LocalEpubItem>,
    pub search_query: String,
    pub filter_status: Option<String>,
    pub is_scanning: bool,
    pub scan_progress: Option<ScanProgressState>,
    pub scan_generation: u64,
    pub batch_progress: Option<BatchProgressState>,
    pub sort_field: EpubSortField,
    pub sort_direction: SortDirection,
}

impl Default for LocalFolderState {
    fn default() -> Self {
        Self {
            current_folder: None,
            recursive_scan: true,
            items: Vec::new(),
            search_query: String::new(),
            filter_status: None,
            is_scanning: false,
            scan_progress: None,
            scan_generation: 0,
            batch_progress: None,
            sort_field: EpubSortField::Title,
            sort_direction: SortDirection::Ascending,
        }
    }
}

/// Compares two strings case-insensitively with zero heap allocations.
pub fn case_insensitive_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
}

/// Compares author names case-insensitively while placing `"Unknown"` last.
pub fn compare_authors_unknown_last(a: &str, b: &str) -> std::cmp::Ordering {
    let a_unknown = a.eq_ignore_ascii_case("unknown");
    let b_unknown = b.eq_ignore_ascii_case("unknown");
    match (a_unknown, b_unknown) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => case_insensitive_cmp(a, b),
    }
}

/// Checks whether `haystack` contains `needle_lower` case-insensitively with zero heap allocations.
///
/// Uses an ASCII byte-window fast path when both inputs are ASCII and falls back to
/// cloned Unicode lowercase iterators for multi-byte UTF-8 inputs.
pub fn contains_ascii_or_unicode_icase(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    if haystack.is_ascii() && needle_lower.is_ascii() {
        let h = haystack.as_bytes();
        let n = needle_lower.as_bytes();
        return h.len() >= n.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n));
    }

    let to_lower: fn(char) -> std::char::ToLowercase = char::to_lowercase;
    let mut needle_iter = needle_lower.chars().flat_map(to_lower);
    let Some(first) = needle_iter.next() else {
        return true;
    };

    let mut hay_iter = haystack.chars().flat_map(to_lower);
    while let Some(hc) = hay_iter.next() {
        if hc == first {
            let mut h_rest = hay_iter.clone();
            let mut matched = true;
            for nc in needle_iter.clone() {
                if h_rest.next() != Some(nc) {
                    matched = false;
                    break;
                }
            }
            if matched {
                return true;
            }
        }
    }
    false
}

impl LocalFolderState {
    pub fn sorted_and_filtered_indices(&self) -> Vec<usize> {
        let search_lower = if self.search_query.is_empty() {
            String::new()
        } else {
            self.search_query.to_lowercase()
        };
        let filter_status = self.filter_status.as_deref();

        let mut indices: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if !search_lower.is_empty()
                    && !contains_ascii_or_unicode_icase(&item.title, &search_lower)
                    && !contains_ascii_or_unicode_icase(&item.author, &search_lower)
                    && !contains_ascii_or_unicode_icase(&item.filename, &search_lower)
                {
                    return false;
                }
                if let Some(st) = filter_status {
                    match st {
                        "unconverted" => item.status == LocalEpubStatus::Unconverted,
                        "in_queue" => item.status == LocalEpubStatus::InQueue,
                        "converted" => item.status == LocalEpubStatus::ConvertedLocal,
                        "on_device" => item.status == LocalEpubStatus::OnDevice,
                        _ => true,
                    }
                } else {
                    true
                }
            })
            .map(|(idx, _)| idx)
            .collect();

        indices.sort_by(|&a_idx, &b_idx| {
            let a = &self.items[a_idx];
            let b = &self.items[b_idx];

            let cmp = match self.sort_field {
                EpubSortField::Title => case_insensitive_cmp(&a.title, &b.title),
                EpubSortField::Author => compare_authors_unknown_last(&a.author, &b.author),
                EpubSortField::DateModified => a.modified_timestamp.cmp(&b.modified_timestamp),
                EpubSortField::FileSize => a.file_size_bytes.cmp(&b.file_size_bytes),
                EpubSortField::Status => a.status.priority().cmp(&b.status.priority()),
            };

            let ordered_cmp = match self.sort_direction {
                SortDirection::Ascending => cmp,
                SortDirection::Descending => cmp.reverse(),
            };

            ordered_cmp
                .then_with(|| a.title.cmp(&b.title))
                .then_with(|| a.id.cmp(&b.id))
        });

        indices
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    Queued,
    Converting,
    Syncing,
    Completed,
    ConvertedLocalOnly,
    SyncFailed(String),
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct ConversionTask {
    pub id: u64,
    pub filename: String,
    pub file_size_str: String,
    pub status: TaskStatus,
    pub progress: f32, // 0.0 to 1.0
    pub word_wise: bool,
}

pub type KindleFile = kfx_converter::KindleBookFile;

#[derive(Debug, Clone)]
pub struct DeviceStatus {
    pub connected: bool,
    pub model: String,
    pub path: Option<PathBuf>,
    pub root_path: Option<PathBuf>,
    pub used_gb: f32,
    pub total_gb: f32,
}

impl Default for DeviceStatus {
    fn default() -> Self {
        Self {
            connected: false,
            model: "Kindle Disconnected".into(),
            path: None,
            root_path: None,
            used_gb: 0.0,
            total_gb: 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConversionSettings {
    pub content_type: String, // "EBOK" or "PDOC"
    pub word_wise: bool,
    pub auto_deploy: bool,
}

impl Default for ConversionSettings {
    fn default() -> Self {
        Self {
            content_type: "EBOK".into(),
            word_wise: true,
            auto_deploy: true,
        }
    }
}

pub(crate) fn device_status_from_info(
    info: Option<kfx_converter::sync::KindleDeviceInfo>,
) -> DeviceStatus {
    if let Some(info) = info {
        DeviceStatus {
            connected: true,
            model: info.model_name,
            path: Some(info.documents_dir),
            root_path: Some(info.root),
            used_gb: info.used_gb,
            total_gb: info.total_gb,
        }
    } else {
        DeviceStatus::default()
    }
}

/// Discovers connected Kindle devices on Linux (GVFS MTP and USB Mass Storage).
pub fn detect_kindle_device() -> DeviceStatus {
    device_status_from_info(kfx_converter::KindleSync::detect_device_info())
}

/// Formats a byte count into a compact human-readable KB/MB string.
pub fn format_file_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KB", bytes / 1024)
    }
}

/// Applies a unified conversion outcome across `tasks`, `folder_items`, `batch_progress`,
/// `device_files`, and `status_message` so batch and single-file runs share identical state transitions.
#[allow(clippy::too_many_arguments)]
pub fn apply_conversion_outcome_to_state(
    tasks: &mut [ConversionTask],
    folder_items: &mut [LocalEpubItem],
    mut batch_progress: Option<&mut BatchProgressState>,
    device_files: &mut Vec<KindleFile>,
    device: &DeviceStatus,
    status_message: &mut Option<(String, bool)>,
    task_id: u64,
    target_path: &Path,
    outcome: ConversionOutcome,
) {
    let was_pending = tasks.iter().find(|t| t.id == task_id).is_some_and(|t| {
        matches!(
            t.status,
            TaskStatus::Queued | TaskStatus::Converting | TaskStatus::Syncing
        )
    });

    match outcome {
        ConversionOutcome::Finished(res) => {
            if let Some(ref ww_err) = res.word_wise_error {
                *status_message = Some((format_task_error("Word Wise", ww_err), true));
            } else if let Some(ref sdr_err) = res.sdr_error {
                *status_message = Some((format_task_error("Sidecar (.sdr)", sdr_err), true));
            } else if !res.synced_to_device
                && let Some(ref sync_err) = res.sync_error
            {
                *status_message = Some((format_task_error("Sync", sync_err), true));
            }

            if let Some(ref mut bp) = batch_progress
                && was_pending
            {
                if res.sync_error.is_some() && bp.is_sync {
                    bp.failed_tasks += 1;
                } else {
                    bp.completed_tasks += 1;
                }
            }

            if let Some(t) = tasks.iter_mut().find(|t| t.id == task_id) {
                t.progress = 1.0;
                t.status = if res.synced_to_device {
                    TaskStatus::Completed
                } else if let Some(ref sync_err) = res.sync_error {
                    TaskStatus::SyncFailed(format_task_error("Sync", sync_err))
                } else {
                    TaskStatus::ConvertedLocalOnly
                };
            }

            if res.synced_to_device {
                let local_name = res
                    .output_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let dev_path = res.synced_device_path.clone().unwrap_or_else(|| {
                    device
                        .path
                        .as_ref()
                        .map(|p| p.join(&local_name))
                        .unwrap_or_else(|| res.output_path.clone())
                });
                let kfx_name = dev_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or(local_name);
                let already_present = device_files.iter().any(|f| f.full_path == dev_path);
                if !already_present {
                    let (item_title, item_author) =
                        if let Some(item) = folder_items.iter().find(|i| i.path == target_path) {
                            (item.title.clone(), item.author.clone())
                        } else {
                            (
                                res.output_path
                                    .file_stem()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string(),
                                "Unknown".into(),
                            )
                        };
                    let now_ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let size_formatted = if res.total_bytes > 0 {
                        format_file_size(res.total_bytes)
                    } else {
                        "Synced".into()
                    };
                    device_files.push(KindleFile {
                        name: kfx_name,
                        title: item_title,
                        author: item_author,
                        format: "KFX".into(),
                        size_formatted,
                        size_bytes: res.total_bytes,
                        modified: "Just now".into(),
                        modified_timestamp: now_ts,
                        full_path: dev_path,
                        has_word_wise: res.word_wise_glosses.is_some_and(|g| g > 0)
                            && res.word_wise_error.is_none()
                            && res.sdr_error.is_none(),
                        has_sdr: true,
                    });
                }
            }

            if let Some(item) = folder_items.iter_mut().find(|i| i.path == target_path) {
                item.status = if res.synced_to_device {
                    LocalEpubStatus::OnDevice
                } else {
                    LocalEpubStatus::ConvertedLocal
                };
                item.is_selected = false;
            }
        }
        ConversionOutcome::Failed(err) => {
            let formatted = format_task_error("Conversion", &err);
            if let Some(ref mut bp) = batch_progress
                && was_pending
            {
                bp.failed_tasks += 1;
            }
            if let Some(t) = tasks.iter_mut().find(|t| t.id == task_id) {
                t.progress = 1.0;
                t.status = TaskStatus::Failed(formatted.clone());
            }
            if let Some(item) = folder_items.iter_mut().find(|i| i.path == target_path) {
                item.status = LocalEpubStatus::Error(formatted.clone());
                item.is_selected = false;
            }
            *status_message = Some((formatted, true));
        }
        ConversionOutcome::Cancelled => {
            if let Some(ref mut bp) = batch_progress
                && was_pending
            {
                bp.failed_tasks += 1;
            }
            if let Some(t) = tasks.iter_mut().find(|t| t.id == task_id) {
                t.progress = 0.0;
                t.status = TaskStatus::Failed("Batch conversion cancelled".into());
            }
            if let Some(item) = folder_items.iter_mut().find(|i| i.path == target_path)
                && matches!(
                    item.status,
                    LocalEpubStatus::InQueue | LocalEpubStatus::Processing { .. }
                )
            {
                item.status = LocalEpubStatus::Unconverted;
            }
        }
    }
}

/// Collects `.epub` files (plus pre-formatted size strings) for an incoming dropped file or folder
/// off the UI thread.
pub fn collect_incoming_epub_paths(path: &Path) -> Vec<(PathBuf, String)> {
    let mut paths = Vec::new();
    if path.is_file() {
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("epub"))
        {
            paths.push(path.to_path_buf());
        }
    } else if path.is_dir() {
        collect_epub_files(path, true, 0, 5, &mut paths, None, None);
        paths.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    }

    paths
        .into_iter()
        .map(|p| {
            let size_str = std::fs::metadata(&p)
                .map(|m| format_file_size(m.len()))
                .unwrap_or_else(|_| "Unknown".into());
            (p, size_str)
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct ConversionResult {
    pub output_path: PathBuf,
    pub total_bytes: u64,
    pub synced_to_device: bool,
    pub synced_device_path: Option<PathBuf>,
    pub sync_error: Option<String>,
    pub word_wise_glosses: Option<usize>,
    pub word_wise_error: Option<String>,
    pub sdr_error: Option<String>,
}

pub(crate) fn build_conversion_options(
    settings: &ConversionSettings,
    device: &DeviceStatus,
) -> kfx_converter::ConversionOptions {
    let should_deploy = settings.auto_deploy && device.connected;
    let device_root = if should_deploy {
        device.root_path.clone().or_else(|| {
            device
                .path
                .as_ref()
                .and_then(|p| p.parent())
                .filter(|pp| !pp.as_os_str().is_empty())
                .map(Path::to_path_buf)
        })
    } else {
        None
    };

    kfx_converter::ConversionOptions {
        content_type: settings.content_type.clone(),
        word_wise: settings.word_wise,
        auto_deploy: should_deploy,
        device_root,
    }
}

/// Executes the complete EPUB to KFX conversion pipeline, including optional
/// baseline cover optimization, Word Wise sidecar (.kll) generation, and Kindle synchronization.
pub fn convert_epub_file<F>(
    input_path: &PathBuf,
    settings: &ConversionSettings,
    device: &DeviceStatus,
    progress: Option<F>,
) -> Result<ConversionResult, String>
where
    F: Fn(f32, &str) + Send + Sync + 'static,
{
    let options = build_conversion_options(settings, device);

    let report = kfx_converter::ConversionPipeline::run(input_path, None, &options, progress)
        .map_err(|e| e.to_string())?;

    Ok(ConversionResult {
        output_path: report.output_path,
        total_bytes: report.total_bytes,
        synced_to_device: report.synced_to_device,
        synced_device_path: report.synced_device_path,
        sync_error: report.sync_error,
        word_wise_glosses: report.word_wise_glosses,
        word_wise_error: report.word_wise_error,
        sdr_error: report.sdr_error,
    })
}

/// Scans the Kindle documents directory (including author subfolders) to load real files stored on the device.
pub fn load_device_files(device: &DeviceStatus) -> Vec<KindleFile> {
    if let Some(doc_path) = &device.path {
        kfx_converter::KindleSync::scan_device_files(doc_path)
    } else {
        Vec::new()
    }
}

/// Deletes an e-book file and its companion .sdr cache directory from the Kindle device.
/// If `allowed_root` is provided, ensures the deletion stays strictly within that directory.
pub fn delete_device_file(allowed_root: Option<&Path>, file: &KindleFile) -> Result<(), String> {
    kfx_converter::KindleSync::delete_book_file_safe(allowed_root, &file.full_path)
        .map_err(|e| format!("Failed to delete book file: {}", e))
}

fn update_scan_progress(
    progress: Option<&std::sync::Arc<std::sync::Mutex<ScanProgressState>>>,
    phase: ScanPhase,
) {
    if let Some(p) = progress {
        let mut guard = match p.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.phase = phase;
    }
}

/// Recursively collects .epub files within dir up to max_depth.
fn collect_epub_files(
    dir: &std::path::Path,
    recursive: bool,
    depth: usize,
    max_depth: usize,
    out: &mut Vec<PathBuf>,
    progress: Option<&std::sync::Arc<std::sync::Mutex<ScanProgressState>>>,
    cancel: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
) {
    if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Acquire)) {
        return;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Acquire)) {
                return;
            }
            let path = entry.path();
            if path.is_file() {
                if path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("epub"))
                    .unwrap_or(false)
                {
                    out.push(path);
                    if out.len() == 1 || out.len().is_multiple_of(10) {
                        update_scan_progress(
                            progress,
                            ScanPhase::Discovering {
                                found_files: out.len(),
                            },
                        );
                    }
                }
            } else if recursive && path.is_dir() && depth < max_depth {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                if !name.starts_with('.') && !name.ends_with(".sdr") {
                    collect_epub_files(
                        &path,
                        recursive,
                        depth + 1,
                        max_depth,
                        out,
                        progress,
                        cancel,
                    );
                }
            }
        }
    }
}

/// Scans a local directory for EPUB files, extracts metadata, and cross-references status with Kindle.
#[cfg(test)]
pub fn scan_local_epub_directory(
    folder: &std::path::Path,
    recursive: bool,
    device: &DeviceStatus,
) -> Vec<LocalEpubItem> {
    scan_local_epub_directory_with_progress(folder, recursive, device, None, None)
}

/// Scans a local directory for EPUB files while reporting progress and honoring cancellation.
pub fn scan_local_epub_directory_with_progress(
    folder: &std::path::Path,
    recursive: bool,
    device: &DeviceStatus,
    progress: Option<&std::sync::Arc<std::sync::Mutex<ScanProgressState>>>,
    cancel: Option<&std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> Vec<LocalEpubItem> {
    let mut files = Vec::new();
    collect_epub_files(folder, recursive, 0, 5, &mut files, progress, cancel);
    if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Acquire)) {
        return Vec::new();
    }
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    let total = files.len();
    update_scan_progress(
        progress,
        ScanPhase::ReadingMetadata {
            processed: 0,
            total,
            current_file: String::new(),
        },
    );

    let device_kfx_stems = if device.connected
        && let Some(ref doc_dir) = device.path
    {
        collect_device_kfx_stems(doc_dir)
    } else {
        std::collections::HashSet::new()
    };

    let mut items = Vec::with_capacity(total);

    for (idx, (id_counter, path)) in (1u64..).zip(files).enumerate() {
        if cancel.is_some_and(|c| c.load(std::sync::atomic::Ordering::Acquire)) {
            return Vec::new();
        }
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        update_scan_progress(
            progress,
            ScanPhase::ReadingMetadata {
                processed: idx,
                total,
                current_file: filename.clone(),
            },
        );
        let meta_res = std::fs::metadata(&path);
        let (file_size_bytes, file_size_str) = match &meta_res {
            Ok(meta) => {
                let bytes = meta.len();
                (bytes, format_file_size(bytes))
            }
            Err(_) => (0, "0 KB".into()),
        };

        let (modified_timestamp, modified_str) =
            match meta_res.as_ref().ok().and_then(|m| m.modified().ok()) {
                Some(time) => {
                    let ts = time
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let duration = time.elapsed().unwrap_or_default();
                    let s = if duration.as_secs() < 60 {
                        "Just now".into()
                    } else if duration.as_secs() < 3600 {
                        format!("{}m ago", duration.as_secs() / 60)
                    } else if duration.as_secs() < 86400 {
                        format!("{}h ago", duration.as_secs() / 3600)
                    } else {
                        format!("{}d ago", duration.as_secs() / 86400)
                    };
                    (ts, s)
                }
                None => (0, "Unknown".into()),
            };

        let (title, author) = match kfx_converter::EpubReader::read_metadata_from_file(&path) {
            Ok(meta) => {
                let t = if meta.title.trim().is_empty() {
                    path.file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string()
                } else {
                    meta.title
                };
                let a = if meta.creators.is_empty() {
                    "Unknown".to_string()
                } else {
                    meta.creators.join(", ")
                };
                (t, a)
            }
            Err(_) => {
                let t = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                (t, "Unknown".to_string())
            }
        };

        // Determine status
        let local_kfx_exists = path.with_extension("kfx").exists();
        let on_kindle = if !device_kfx_stems.is_empty() {
            let raw_stem = path.file_stem().unwrap_or_default().to_string_lossy();
            let raw_stem_lower = raw_stem.to_lowercase();
            let sanitized_stem_lower =
                kfx_converter::KindleSync::sanitize_filename(&raw_stem).to_lowercase();
            let sanitized_title_lower =
                kfx_converter::KindleSync::sanitize_filename(&title).to_lowercase();
            device_kfx_stems.contains(&raw_stem_lower)
                || device_kfx_stems.contains(&sanitized_stem_lower)
                || device_kfx_stems.contains(&sanitized_title_lower)
        } else {
            false
        };

        let status = if on_kindle {
            LocalEpubStatus::OnDevice
        } else if local_kfx_exists {
            LocalEpubStatus::ConvertedLocal
        } else {
            LocalEpubStatus::Unconverted
        };

        items.push(LocalEpubItem {
            id: id_counter,
            path,
            filename,
            title,
            author,
            file_size_bytes,
            file_size_str,
            modified_timestamp,
            modified_str,
            is_selected: false,
            status,
        });
    }

    items
}

/// Safely unmounts and ejects the connected Kindle device.
pub fn eject_kindle_device(device: &DeviceStatus) -> Result<(), String> {
    let root = match (&device.root_path, &device.path) {
        (Some(r), _) => r.as_path(),
        (None, Some(p)) => p.parent().unwrap_or(p.as_path()),
        _ => return Err("No Kindle device connected.".into()),
    };
    kfx_converter::KindleSync::eject_device(root).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_minimal_epub(path: &Path, title: &str, author: &str) {
        use std::io::Write;
        let file = std::fs::File::create(path).expect("Create minimal epub");
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);

        zip.start_file("mimetype", opts).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();

        zip.start_file("META-INF/container.xml", opts).unwrap();
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .unwrap();

        let opf = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>{title}</dc:title>
    <dc:creator>{author}</dc:creator>
    <dc:language>en</dc:language>
  </metadata>
  <manifest/>
  <spine/>
</package>"#
        );
        zip.start_file("OEBPS/content.opf", opts).unwrap();
        zip.write_all(opf.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn test_detect_kindle_device_and_load_files() {
        // 1. Disconnected mapping yields default status
        let disconnected = device_status_from_info(None);
        assert!(!disconnected.connected);
        assert!(disconnected.path.is_none());
        assert!(disconnected.root_path.is_none());

        // 2. Deterministic TempDir mock Kindle with top-level, author subfolder, depth-3 subfolder,
        //    companion .sdr Word Wise sidecar, depth-4 exclusion, and Unix symlink exclusion
        let temp_dir = tempfile::tempdir().expect("Create mock Kindle temp dir");
        let root = temp_dir.path();
        let docs_dir = root.join("documents");
        let sys_dir = root.join("system");
        let author_dir = docs_dir.join("Frank Herbert");
        let depth3_dir = docs_dir.join("Level1").join("Level2").join("Level3");
        let depth4_dir = depth3_dir.join("Level4");
        let top_sdr = docs_dir.join("TopBook.sdr");

        std::fs::create_dir_all(&sys_dir).unwrap();
        std::fs::create_dir_all(&author_dir).unwrap();
        std::fs::create_dir_all(&depth4_dir).unwrap();
        std::fs::create_dir_all(&top_sdr).unwrap();

        std::fs::write(sys_dir.join("version.txt"), "Kindle 5.16.21").unwrap();
        std::fs::write(docs_dir.join("TopBook.kfx"), b"CONT top").unwrap();
        std::fs::write(top_sdr.join("LanguageLayer.en.B00TOP.kll"), b"kll db").unwrap();
        std::fs::write(author_dir.join("SubBook.kfx"), b"CONT sub").unwrap();
        std::fs::write(depth3_dir.join("DepthThree.kfx"), b"CONT d3").unwrap();
        std::fs::write(depth4_dir.join("TooDeep.kfx"), b"CONT d4").unwrap();
        std::fs::write(docs_dir.join("LegacyBook.azw3"), b"MOBI").unwrap();

        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().expect("Create outside dir");
            let outside_kfx = outside.path().join("SymlinkTarget.kfx");
            std::fs::write(&outside_kfx, b"CONT outside").unwrap();
            std::os::unix::fs::symlink(&outside_kfx, docs_dir.join("LinkedBook.kfx")).unwrap();
            std::os::unix::fs::symlink(outside.path(), docs_dir.join("LinkedDir")).unwrap();

            let stems_with_symlinks = collect_device_kfx_stems(&docs_dir);
            assert!(!stems_with_symlinks.contains("linkedbook"));
            assert!(!stems_with_symlinks.contains("symlinktarget"));
        }

        let (used_gb, total_gb) =
            kfx_converter::KindleSync::get_storage_stats(root).unwrap_or((0.0, 0.0));
        assert!(
            total_gb > 0.0,
            "Mock Kindle temp filesystem must report > 0 GB"
        );
        assert!(used_gb >= 0.0);

        let info = kfx_converter::sync::KindleDeviceInfo {
            root: root.to_path_buf(),
            documents_dir: docs_dir.clone(),
            model_name: kfx_converter::KindleSync::resolve_model_name(root, "Kindle Paperwhite"),
            used_gb,
            total_gb,
        };
        let device = device_status_from_info(Some(info));
        assert!(device.connected);
        assert_eq!(device.path.as_deref(), Some(docs_dir.as_path()));
        assert_eq!(device.root_path.as_deref(), Some(root));
        assert!(device.model.starts_with("Kindle"));
        assert!(device.total_gb > 0.0);

        let files = load_device_files(&device);
        assert_eq!(files.len(), 4);
        let top = files.iter().find(|f| f.name == "TopBook.kfx").unwrap();
        assert!(top.has_sdr);
        assert!(top.has_word_wise);
        let sub = files.iter().find(|f| f.name == "SubBook.kfx").unwrap();
        assert_eq!(sub.author, "Frank Herbert");
        assert!(files.iter().any(|f| f.name == "DepthThree.kfx"));
        assert!(!files.iter().any(|f| f.name == "TooDeep.kfx"));

        let stems = collect_device_kfx_stems(&docs_dir);
        assert!(stems.contains("topbook"));
        assert!(stems.contains("subbook"));
        assert!(stems.contains("depththree"));
        assert!(!stems.contains("toodeep"));
        assert!(!stems.contains("legacybook"));
    }

    #[test]
    fn test_scan_local_epub_matches_subfolder_and_sanitized_raw_stem() {
        let temp_dir = tempfile::tempdir().expect("Create temp dir");
        let epub_dir = temp_dir.path().join("epubs");
        let doc_dir = temp_dir.path().join("kindle").join("documents");
        let author_dir = doc_dir.join("Isaac Asimov");
        std::fs::create_dir_all(&epub_dir).unwrap();
        std::fs::create_dir_all(&author_dir).unwrap();

        // Device holds sanitized .kfx inside an author subfolder (depth 1)
        std::fs::write(author_dir.join("Book_Part 1.kfx"), b"CONT").unwrap();
        // Local EPUB has a filename stem ("Book..Part 1") that sanitizes to "Book_Part 1"
        // while its OPF <dc:title> is completely different so only sanitized_stem_lower matches.
        write_minimal_epub(
            &epub_dir.join("Book..Part 1.epub"),
            "Completely Different Metadata Title",
            "Isaac Asimov",
        );
        write_minimal_epub(
            &epub_dir.join("NotSynced.epub"),
            "Not Synced Title",
            "Isaac Asimov",
        );

        #[cfg(unix)]
        {
            // On Unix, also verify colon sanitization ("Book: Part 1.epub" -> "Book_ Part 1.kfx")
            std::fs::write(author_dir.join("Book_ Part 1.kfx"), b"CONT").unwrap();
            write_minimal_epub(
                &epub_dir.join("Book: Part 1.epub"),
                "Another Distinct Title",
                "Isaac Asimov",
            );
        }

        let dev = DeviceStatus {
            connected: true,
            model: "Kindle Paperwhite".into(),
            path: Some(doc_dir),
            root_path: Some(temp_dir.path().join("kindle")),
            used_gb: 1.0,
            total_gb: 16.0,
        };

        let items = scan_local_epub_directory_with_progress(&epub_dir, false, &dev, None, None);
        let dot_item = items
            .iter()
            .find(|i| i.filename == "Book..Part 1.epub")
            .unwrap();
        assert_eq!(dot_item.title, "Completely Different Metadata Title");
        assert_eq!(dot_item.status, LocalEpubStatus::OnDevice);

        let unsynced = items
            .iter()
            .find(|i| i.filename == "NotSynced.epub")
            .unwrap();
        assert_eq!(unsynced.status, LocalEpubStatus::Unconverted);

        #[cfg(unix)]
        {
            let colon_item = items
                .iter()
                .find(|i| i.filename == "Book: Part 1.epub")
                .unwrap();
            assert_eq!(colon_item.title, "Another Distinct Title");
            assert_eq!(colon_item.status, LocalEpubStatus::OnDevice);
        }
    }

    #[test]
    fn test_load_and_delete_device_files_with_word_wise_sdr() {
        let temp_dir = tempfile::tempdir().expect("Create temp dir");
        let docs_dir = temp_dir.path().join("documents");
        std::fs::create_dir_all(&docs_dir).expect("Create documents dir");

        let book_path = docs_dir.join("TestBook.kfx");
        std::fs::write(&book_path, b"dummy kfx data").expect("Write test kfx");

        let sdr_path = docs_dir.join("TestBook.sdr");
        std::fs::create_dir_all(&sdr_path).expect("Create test sdr dir");
        std::fs::write(
            sdr_path.join("LanguageLayer.en.B00TEST.kll"),
            b"dummy kll db",
        )
        .expect("Write test kll");

        let dev = DeviceStatus {
            connected: true,
            model: "Kindle Scribe".to_string(),
            path: Some(docs_dir.clone()),
            root_path: Some(temp_dir.path().to_path_buf()),
            used_gb: 1.0,
            total_gb: 16.0,
        };

        let files = load_device_files(&dev);
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!(f.name, "TestBook.kfx");
        assert_eq!(f.format, "KFX");
        assert!(f.has_sdr);
        assert!(f.has_word_wise);

        // Test delete_device_file with root confinement
        delete_device_file(dev.path.as_deref(), f).expect("Delete device file within allowed root");
        assert!(!book_path.exists());
        assert!(!sdr_path.exists());

        // Test delete_device_file rejects files outside allowed root
        let outside_temp = tempfile::tempdir().expect("Create outside temp dir");
        let outside_book = outside_temp.path().join("OutsideBook.kfx");
        std::fs::write(&outside_book, b"dummy outside content").expect("Write outside book");
        let outside_f = KindleFile {
            name: "OutsideBook.kfx".into(),
            title: "OutsideBook".into(),
            author: "".into(),
            format: "KFX".into(),
            size_formatted: "1 KB".into(),
            size_bytes: 20,
            modified: "".into(),
            modified_timestamp: 0,
            full_path: outside_book.clone(),
            has_word_wise: false,
            has_sdr: false,
        };
        let err_res = delete_device_file(dev.path.as_deref(), &outside_f);
        assert!(
            err_res.is_err(),
            "Must reject deletion outside allowed_root"
        );
        assert!(outside_book.exists(), "Outside file must not be deleted");

        let files_after = load_device_files(&dev);
        assert_eq!(files_after.len(), 0);
    }

    #[test]
    fn test_load_device_files_recursive_author_subfolders() {
        let temp_dir = tempfile::tempdir().expect("Create temp dir");
        let docs_dir = temp_dir.path().join("documents");
        let author_dir = docs_dir.join("Brandon Sanderson");
        let nested_sdr = author_dir.join("WayOfKings.sdr");
        std::fs::create_dir_all(&nested_sdr).expect("Create author and sdr dir");

        let book1 = author_dir.join("WayOfKings.azw3");
        std::fs::write(&book1, b"azw3 content").expect("Write nested azw3");
        std::fs::write(nested_sdr.join("LanguageLayer.en.B00WAY.kll"), b"kll db")
            .expect("Write kll");

        let store_dir = docs_dir.join("Downloads").join("Items01");
        std::fs::create_dir_all(&store_dir).expect("Create store dir");
        let book2 = store_dir.join("StoreItem.kfx");
        std::fs::write(&book2, b"kfx content").expect("Write store kfx");

        let dev = DeviceStatus {
            connected: true,
            model: "Kindle Paperwhite".to_string(),
            path: Some(docs_dir),
            root_path: Some(temp_dir.path().to_path_buf()),
            used_gb: 2.5,
            total_gb: 16.0,
        };

        let files = load_device_files(&dev);
        assert_eq!(files.len(), 2);

        let way_of_kings = files.iter().find(|f| f.name == "WayOfKings.azw3").unwrap();
        assert_eq!(way_of_kings.format, "AZW3");
        assert_eq!(way_of_kings.author, "Brandon Sanderson");
        assert_eq!(way_of_kings.title, "WayOfKings");
        assert!(way_of_kings.has_sdr);
        assert!(way_of_kings.has_word_wise);

        let store_item = files.iter().find(|f| f.name == "StoreItem.kfx").unwrap();
        assert_eq!(store_item.format, "KFX");
        assert_eq!(store_item.author, "Unknown");
        assert_eq!(store_item.title, "StoreItem");
        assert!(!store_item.has_sdr);
        assert!(!store_item.has_word_wise);
    }

    #[test]
    fn test_scan_local_epub_directory_and_status() {
        let temp_dir = tempfile::tempdir().expect("Create temp dir");
        let epub_dir = temp_dir.path().join("my_epubs");
        let sub_dir = epub_dir.join("sci_fi");
        std::fs::create_dir_all(&sub_dir).expect("Create sub dir");

        let epub1 = epub_dir.join("BookOne.epub");
        std::fs::write(&epub1, b"PK\x03\x04dummy epub 1").expect("Write epub1");

        let epub2 = sub_dir.join("BookTwo.epub");
        std::fs::write(&epub2, b"PK\x03\x04dummy epub 2").expect("Write epub2");

        // Create a local .kfx for BookOne to test ConvertedLocal status
        let kfx1 = epub_dir.join("BookOne.kfx");
        std::fs::write(&kfx1, b"CONT dummy").expect("Write kfx1");

        let dummy_dev = DeviceStatus::default();

        // 1. Recursive scan = true
        let items_rec = scan_local_epub_directory(&epub_dir, true, &dummy_dev);
        assert_eq!(items_rec.len(), 2);

        let item1 = items_rec
            .iter()
            .find(|i| i.filename == "BookOne.epub")
            .unwrap();
        assert_eq!(item1.status, LocalEpubStatus::ConvertedLocal);

        let item2 = items_rec
            .iter()
            .find(|i| i.filename == "BookTwo.epub")
            .unwrap();
        assert_eq!(item2.status, LocalEpubStatus::Unconverted);

        // 2. Recursive scan = false
        let items_flat = scan_local_epub_directory(&epub_dir, false, &dummy_dev);
        assert_eq!(items_flat.len(), 1);
        assert_eq!(items_flat[0].filename, "BookOne.epub");
    }

    #[test]
    fn test_batch_progress_state_and_processing_status() {
        let mut bp = BatchProgressState {
            total_tasks: 10,
            completed_tasks: 3,
            failed_tasks: 1,
            active_tasks: 2,
            current_filename: "Originals.epub".into(),
            current_stage: "Packaging KFX".into(),
            is_sync: true,
            is_cancelled: false,
        };

        // Area 4 approved update: failed_tasks are included in overall_progress numerator
        assert_eq!(bp.overall_percentage(), 40);
        assert!((bp.overall_progress() - 0.4).abs() < 1e-5);

        bp.completed_tasks = 9;
        assert_eq!(bp.overall_percentage(), 100);
        assert!((bp.overall_progress() - 1.0).abs() < 1e-5);

        let empty_bp = BatchProgressState::default();
        assert_eq!(empty_bp.overall_progress(), 0.0);

        let proc_status = LocalEpubStatus::Processing {
            stage: "Ingesting EPUB".into(),
            progress: 15,
        };
        assert!(matches!(
            proc_status,
            LocalEpubStatus::Processing { progress: 15, .. }
        ));
    }

    #[test]
    fn test_sorted_and_filtered_indices() {
        let mut state = LocalFolderState {
            items: vec![
                LocalEpubItem {
                    id: 1,
                    path: PathBuf::from("/books/Zeta.epub"),
                    filename: "Zeta.epub".into(),
                    title: "Zeta Book".into(),
                    author: "Brandon Sanderson".into(),
                    file_size_bytes: 5_000_000,
                    file_size_str: "5.0 MB".into(),
                    modified_timestamp: 100,
                    modified_str: "100s ago".into(),
                    is_selected: false,
                    status: LocalEpubStatus::ConvertedLocal,
                },
                LocalEpubItem {
                    id: 2,
                    path: PathBuf::from("/books/Alpha.epub"),
                    filename: "Alpha.epub".into(),
                    title: "Alpha Book".into(),
                    author: "Arthur Conan Doyle".into(),
                    file_size_bytes: 1_000_000,
                    file_size_str: "1.0 MB".into(),
                    modified_timestamp: 300,
                    modified_str: "300s ago".into(),
                    is_selected: false,
                    status: LocalEpubStatus::Unconverted,
                },
                LocalEpubItem {
                    id: 3,
                    path: PathBuf::from("/books/Beta.epub"),
                    filename: "Beta.epub".into(),
                    title: "Beta Book".into(),
                    author: "Unknown".into(),
                    file_size_bytes: 10_000_000,
                    file_size_str: "10.0 MB".into(),
                    modified_timestamp: 200,
                    modified_str: "200s ago".into(),
                    is_selected: false,
                    status: LocalEpubStatus::OnDevice,
                },
            ],
            ..Default::default()
        };

        // 1. Sort by Title Ascending (Alpha -> Beta -> Zeta)
        state.sort_field = EpubSortField::Title;
        state.sort_direction = SortDirection::Ascending;
        let sorted = state.sorted_and_filtered_indices();
        assert_eq!(sorted, vec![1, 2, 0]); // Alpha (idx 1), Beta (idx 2), Zeta (idx 0)

        // 2. Sort by Title Descending (Zeta -> Beta -> Alpha)
        state.sort_direction = SortDirection::Descending;
        let sorted_desc = state.sorted_and_filtered_indices();
        assert_eq!(sorted_desc, vec![0, 2, 1]);

        // 3. Sort by Author Ascending (Arthur -> Brandon -> Unknown at end)
        state.sort_field = EpubSortField::Author;
        state.sort_direction = SortDirection::Ascending;
        let sorted_author = state.sorted_and_filtered_indices();
        assert_eq!(sorted_author, vec![1, 0, 2]);

        // 4. Sort by FileSize Descending (Beta 10MB -> Zeta 5MB -> Alpha 1MB)
        state.sort_field = EpubSortField::FileSize;
        state.sort_direction = SortDirection::Descending;
        let sorted_size = state.sorted_and_filtered_indices();
        assert_eq!(sorted_size, vec![2, 0, 1]);

        // 5. Sort by DateModified Descending (Alpha 300 -> Beta 200 -> Zeta 100)
        state.sort_field = EpubSortField::DateModified;
        state.sort_direction = SortDirection::Descending;
        let sorted_date = state.sorted_and_filtered_indices();
        assert_eq!(sorted_date, vec![1, 2, 0]);

        // 6. Sort by Status Ascending (Unconverted [1] -> ConvertedLocal [0] -> OnDevice [2])
        state.sort_field = EpubSortField::Status;
        state.sort_direction = SortDirection::Ascending;
        let sorted_status = state.sorted_and_filtered_indices();
        assert_eq!(sorted_status, vec![1, 0, 2]);

        // 7. Filter status + sort
        state.filter_status = Some("unconverted".into());
        let filtered = state.sorted_and_filtered_indices();
        assert_eq!(filtered, vec![1]);
    }

    #[test]
    fn test_scan_progress_state_and_cancellation() {
        let sp = ScanProgressState::discovering();
        assert_eq!(sp.percentage(), 0);
        assert_eq!(sp.fraction(), 0.0);

        let sp_meta = ScanProgressState {
            phase: ScanPhase::ReadingMetadata {
                processed: 2,
                total: 4,
                current_file: "book.epub".into(),
            },
        };
        assert_eq!(sp_meta.percentage(), 50);
        assert!((sp_meta.fraction() - 0.5).abs() < 1e-5);

        let temp_dir = tempfile::tempdir().expect("Create temp dir");
        std::fs::write(temp_dir.path().join("A.epub"), b"dummy1").unwrap();
        std::fs::write(temp_dir.path().join("B.epub"), b"dummy2").unwrap();

        let dummy_dev = DeviceStatus {
            connected: false,
            model: "None".into(),
            used_gb: 0.0,
            total_gb: 0.0,
            path: None,
            root_path: None,
        };

        let progress = std::sync::Arc::new(std::sync::Mutex::new(ScanProgressState::discovering()));
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let items = scan_local_epub_directory_with_progress(
            temp_dir.path(),
            true,
            &dummy_dev,
            Some(&progress),
            Some(&cancel),
        );
        assert_eq!(items.len(), 2);

        let final_phase = progress.lock().unwrap().phase.clone();
        assert!(matches!(
            final_phase,
            ScanPhase::ReadingMetadata { total: 2, .. }
        ));

        // Pre-cancelled scan returns empty immediately
        cancel.store(true, std::sync::atomic::Ordering::Release);
        let cancelled_items = scan_local_epub_directory_with_progress(
            temp_dir.path(),
            true,
            &dummy_dev,
            Some(&progress),
            Some(&cancel),
        );
        assert!(cancelled_items.is_empty());
    }

    #[test]
    fn test_collect_device_kfx_stems_and_sanitized_title_matching() {
        let temp_dir = tempfile::tempdir().expect("Create temp dir");
        let epub_dir = temp_dir.path().join("epubs");
        let doc_dir = temp_dir.path().join("kindle_docs");
        std::fs::create_dir_all(&epub_dir).unwrap();
        std::fs::create_dir_all(&doc_dir).unwrap();

        // Device holds lowercase/sanitized KFX stems, plus a directory that happens to end with .kfx
        std::fs::write(doc_dir.join("Book_CON.kfx"), b"CONT").unwrap();
        std::fs::write(doc_dir.join("My_Great_Book.kfx"), b"CONT").unwrap();
        std::fs::create_dir_all(doc_dir.join("IgnoreDir.kfx")).unwrap();

        let stems = collect_device_kfx_stems(&doc_dir);
        assert!(stems.contains("book_con"));
        assert!(stems.contains("my_great_book"));
        assert!(!stems.contains("ignoredir"));

        // Local EPUB named "CON.epub" (no valid EPUB zip -> title falls back to stem "CON",
        // which sanitizes via KindleSync::sanitize_filename("CON") == "book_CON")
        std::fs::write(epub_dir.join("CON.epub"), b"not-a-zip").unwrap();
        std::fs::write(epub_dir.join("Unmatched.epub"), b"not-a-zip").unwrap();

        let dev = DeviceStatus {
            connected: true,
            model: "Kindle Paperwhite".into(),
            path: Some(doc_dir),
            root_path: Some(temp_dir.path().to_path_buf()),
            used_gb: 1.0,
            total_gb: 8.0,
        };

        let items = scan_local_epub_directory(&epub_dir, false, &dev);
        assert_eq!(items.len(), 2);

        let con_item = items.iter().find(|i| i.filename == "CON.epub").unwrap();
        assert_eq!(con_item.status, LocalEpubStatus::OnDevice);

        let unmatched = items
            .iter()
            .find(|i| i.filename == "Unmatched.epub")
            .unwrap();
        assert_eq!(unmatched.status, LocalEpubStatus::Unconverted);
    }

    #[test]
    fn test_format_task_error_utf8_safe_and_context() {
        assert_eq!(format_task_error("Sync", "   \n  "), "Sync: Unknown error");
        assert_eq!(
            format_task_error("Conversion", "Conversion: already prefixed\nline2"),
            "Conversion: already prefixed line2"
        );

        let long_korean = "가나다라마바사".repeat(40); // multi-byte UTF-8 > 160 chars
        let formatted = format_task_error("Conversion", &long_korean);
        assert!(formatted.starts_with("Conversion: "));
        assert!(formatted.ends_with("..."));
        assert_eq!(formatted.chars().count(), 160);
    }

    #[test]
    fn test_resolve_output_folder_target_validation() {
        let temp_dir = tempfile::tempdir().expect("Create temp dir");
        let sub_dir = temp_dir.path().join("-leading-dash-folder");
        std::fs::create_dir_all(&sub_dir).unwrap();
        let file_path = sub_dir.join("output.kfx");
        std::fs::write(&file_path, b"CONT").unwrap();

        let from_file = resolve_output_folder_target(&file_path).expect("Resolve from file");
        assert!(from_file.is_dir());
        assert!(!from_file.to_string_lossy().starts_with('-'));

        let from_dir = resolve_output_folder_target(&sub_dir).expect("Resolve from dir");
        assert_eq!(from_file, from_dir);

        let missing = temp_dir.path().join("does_not_exist");
        assert!(resolve_output_folder_target(&missing).is_err());
        assert!(resolve_output_folder_target(Path::new("")).is_err());
    }

    #[test]
    fn test_apply_conversion_outcome_to_state_unified() {
        let book_a = PathBuf::from("/books/A.epub");
        let book_b = PathBuf::from("/books/B.epub");
        let mut tasks = vec![
            ConversionTask {
                id: 1,
                filename: "A.epub".into(),
                file_size_str: "10 KB".into(),
                status: TaskStatus::Converting,
                progress: 0.3,
                word_wise: true,
            },
            ConversionTask {
                id: 2,
                filename: "B.epub".into(),
                file_size_str: "20 KB".into(),
                status: TaskStatus::Queued,
                progress: 0.0,
                word_wise: true,
            },
        ];
        let mut items = vec![
            LocalEpubItem {
                id: 1,
                path: book_a.clone(),
                filename: "A.epub".into(),
                title: "Book A".into(),
                author: "Author A".into(),
                file_size_bytes: 10240,
                file_size_str: "10 KB".into(),
                modified_timestamp: 0,
                modified_str: "Just now".into(),
                is_selected: true,
                status: LocalEpubStatus::Processing {
                    stage: "Packaging".into(),
                    progress: 30,
                },
            },
            LocalEpubItem {
                id: 2,
                path: book_b.clone(),
                filename: "B.epub".into(),
                title: "Book B".into(),
                author: "Author B".into(),
                file_size_bytes: 20480,
                file_size_str: "20 KB".into(),
                modified_timestamp: 0,
                modified_str: "Just now".into(),
                is_selected: true,
                status: LocalEpubStatus::InQueue,
            },
        ];
        let mut bp = BatchProgressState {
            total_tasks: 2,
            completed_tasks: 0,
            failed_tasks: 0,
            active_tasks: 1,
            current_filename: "A.epub".into(),
            current_stage: "Packaging".into(),
            is_sync: true,
            is_cancelled: false,
        };
        let dev = DeviceStatus {
            connected: true,
            model: "Kindle".into(),
            path: Some(PathBuf::from("/kindle/documents")),
            root_path: Some(PathBuf::from("/kindle")),
            used_gb: 1.0,
            total_gb: 8.0,
        };
        let mut dev_files = Vec::new();
        let mut status_msg = None;

        // 1. Sync failure with Word Wise warning preserves local KFX status, clears selection, records failure
        apply_conversion_outcome_to_state(
            &mut tasks,
            &mut items,
            Some(&mut bp),
            &mut dev_files,
            &dev,
            &mut status_msg,
            1,
            &book_a,
            ConversionOutcome::Finished(ConversionResult {
                output_path: PathBuf::from("/books/A.kfx"),
                total_bytes: 4096,
                synced_to_device: false,
                synced_device_path: None,
                sync_error: Some("MTP write timeout".into()),
                word_wise_glosses: Some(0),
                word_wise_error: Some("KLL failed".into()),
                sdr_error: None,
            }),
        );
        assert!(matches!(tasks[0].status, TaskStatus::SyncFailed(_)));
        assert_eq!(items[0].status, LocalEpubStatus::ConvertedLocal);
        assert!(!items[0].is_selected);
        assert_eq!(bp.failed_tasks, 1);
        assert_eq!(bp.completed_tasks, 0);
        assert!(
            status_msg
                .as_ref()
                .is_some_and(|(m, is_err)| *is_err && m.contains("Word Wise:"))
        );

        // 2. Cancelled outcome transitions queued task & item cleanly
        apply_conversion_outcome_to_state(
            &mut tasks,
            &mut items,
            Some(&mut bp),
            &mut dev_files,
            &dev,
            &mut status_msg,
            2,
            &book_b,
            ConversionOutcome::Cancelled,
        );
        assert!(matches!(tasks[1].status, TaskStatus::Failed(_)));
        assert_eq!(items[1].status, LocalEpubStatus::Unconverted);
        assert_eq!(bp.failed_tasks, 2);
        assert!((bp.overall_progress() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_contains_ascii_or_unicode_icase_parity_and_edge_cases() {
        // Empty needle always matches
        assert!(contains_ascii_or_unicode_icase("", ""));
        assert!(contains_ascii_or_unicode_icase("Dune", ""));

        // Empty or shorter haystack
        assert!(!contains_ascii_or_unicode_icase("", "dune"));
        assert!(!contains_ascii_or_unicode_icase("Du", "dune"));

        // ASCII fast path
        assert!(contains_ascii_or_unicode_icase(
            "The Way of Kings.epub",
            "way of kings"
        ));
        assert!(contains_ascii_or_unicode_icase("Brandon Sanderson", "sand"));
        assert!(!contains_ascii_or_unicode_icase(
            "Brandon Sanderson",
            "tolkien"
        ));

        // Unicode accented + multi-byte CJK/Korean + Turkish dotted I
        let pairs = [
            ("Café au Lait", "café", true),
            ("Café au Lait", "LAIT", true),
            ("Übermensch", "über", true),
            ("한글 EPUB 테스트", "epub", true),
            ("한글 EPUB 테스트", "테스트", true),
            ("한글 EPUB 테스트", "소설", false),
            ("İstanbul Chronicles", "i\u{307}stanbul", true),
            ("Kelvin Key", "key", true),
            ("Ελληνικά Βιβλία", "ελλην", true),
            ("Ελληνικά Βιβλία", "αθήνα", false),
        ];

        for (haystack, needle, expected) in pairs {
            let needle_lower = needle.to_lowercase();
            assert_eq!(
                contains_ascii_or_unicode_icase(haystack, &needle_lower),
                expected,
                "failed for haystack={haystack:?}, needle={needle:?}"
            );
            assert_eq!(
                contains_ascii_or_unicode_icase(haystack, &needle_lower),
                haystack.to_lowercase().contains(&needle_lower),
                "parity mismatch for haystack={haystack:?}, needle={needle:?}"
            );
        }
    }

    #[test]
    fn test_compare_authors_unknown_last() {
        assert_eq!(
            compare_authors_unknown_last("Unknown", "Arthur"),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            compare_authors_unknown_last("Arthur", "unknown"),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            compare_authors_unknown_last("Unknown", "unknown"),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            compare_authors_unknown_last("alice", "Bob"),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn test_build_conversion_options_wires_device_root() {
        let settings_on = ConversionSettings {
            content_type: "EBOK".into(),
            word_wise: true,
            auto_deploy: true,
        };
        let settings_off = ConversionSettings {
            auto_deploy: false,
            ..settings_on.clone()
        };

        // 1. Explicit root_path is wired directly when auto_deploy && connected
        let dev_with_root = DeviceStatus {
            connected: true,
            model: "Kindle".into(),
            path: Some(PathBuf::from("/mnt/kindle/documents")),
            root_path: Some(PathBuf::from("/mnt/kindle")),
            used_gb: 1.0,
            total_gb: 16.0,
        };
        let opts = build_conversion_options(&settings_on, &dev_with_root);
        assert!(opts.auto_deploy);
        assert_eq!(opts.device_root, Some(PathBuf::from("/mnt/kindle")));

        // 2. Missing root_path falls back to parent of device.path
        let dev_path_only = DeviceStatus {
            root_path: None,
            ..dev_with_root.clone()
        };
        let opts_fallback = build_conversion_options(&settings_on, &dev_path_only);
        assert!(opts_fallback.auto_deploy);
        assert_eq!(
            opts_fallback.device_root,
            Some(PathBuf::from("/mnt/kindle"))
        );

        // 3. auto_deploy == false clears both auto_deploy and device_root
        let opts_no_deploy = build_conversion_options(&settings_off, &dev_with_root);
        assert!(!opts_no_deploy.auto_deploy);
        assert!(opts_no_deploy.device_root.is_none());

        // 4. Disconnected device clears both auto_deploy and device_root
        let dev_disconnected = DeviceStatus {
            connected: false,
            ..dev_with_root
        };
        let opts_disconnected = build_conversion_options(&settings_on, &dev_disconnected);
        assert!(!opts_disconnected.auto_deploy);
        assert!(opts_disconnected.device_root.is_none());
    }

    #[test]
    fn test_apply_conversion_outcome_uses_synced_device_path_total_bytes_and_sdr_error() {
        let epub_con = PathBuf::from("/books/CON.epub");
        let epub_sdr = PathBuf::from("/books/SdrWarn.epub");
        let mut tasks = vec![
            ConversionTask {
                id: 1,
                filename: "CON.epub".into(),
                file_size_str: "10 KB".into(),
                status: TaskStatus::Converting,
                progress: 0.5,
                word_wise: true,
            },
            ConversionTask {
                id: 2,
                filename: "SdrWarn.epub".into(),
                file_size_str: "12 KB".into(),
                status: TaskStatus::Converting,
                progress: 0.5,
                word_wise: true,
            },
        ];
        let mut items = vec![
            LocalEpubItem {
                id: 1,
                path: epub_con.clone(),
                filename: "CON.epub".into(),
                title: "CON".into(),
                author: "Author".into(),
                file_size_bytes: 10240,
                file_size_str: "10 KB".into(),
                modified_timestamp: 0,
                modified_str: "Just now".into(),
                is_selected: true,
                status: LocalEpubStatus::Processing {
                    stage: "Sync".into(),
                    progress: 50,
                },
            },
            LocalEpubItem {
                id: 2,
                path: epub_sdr.clone(),
                filename: "SdrWarn.epub".into(),
                title: "SdrWarn".into(),
                author: "Author".into(),
                file_size_bytes: 12288,
                file_size_str: "12 KB".into(),
                modified_timestamp: 0,
                modified_str: "Just now".into(),
                is_selected: true,
                status: LocalEpubStatus::Processing {
                    stage: "Sync".into(),
                    progress: 50,
                },
            },
        ];
        let dev = DeviceStatus {
            connected: true,
            model: "Kindle".into(),
            path: Some(PathBuf::from("/kindle/documents")),
            root_path: Some(PathBuf::from("/kindle")),
            used_gb: 1.0,
            total_gb: 8.0,
        };
        let mut dev_files = Vec::new();
        let mut status_msg = None;

        // 1. Sanitized synced_device_path and non-zero total_bytes are preserved
        let sanitized_dev_path = PathBuf::from("/kindle/documents/book_CON.kfx");
        apply_conversion_outcome_to_state(
            &mut tasks,
            &mut items,
            None,
            &mut dev_files,
            &dev,
            &mut status_msg,
            1,
            &epub_con,
            ConversionOutcome::Finished(ConversionResult {
                output_path: PathBuf::from("/books/CON.kfx"),
                total_bytes: 2048,
                synced_to_device: true,
                synced_device_path: Some(sanitized_dev_path.clone()),
                sync_error: None,
                word_wise_glosses: Some(25),
                word_wise_error: None,
                sdr_error: None,
            }),
        );
        assert_eq!(dev_files.len(), 1);
        assert_eq!(dev_files[0].full_path, sanitized_dev_path);
        assert_eq!(dev_files[0].name, "book_CON.kfx");
        assert_eq!(dev_files[0].size_bytes, 2048);
        assert_eq!(dev_files[0].size_formatted, "2 KB");
        assert!(dev_files[0].has_word_wise);
        assert!(status_msg.is_none());

        // 2. sdr_error surfaces cleanly and suppresses has_word_wise while 0 bytes falls back to "Synced"
        apply_conversion_outcome_to_state(
            &mut tasks,
            &mut items,
            None,
            &mut dev_files,
            &dev,
            &mut status_msg,
            2,
            &epub_sdr,
            ConversionOutcome::Finished(ConversionResult {
                output_path: PathBuf::from("/books/SdrWarn.kfx"),
                total_bytes: 0,
                synced_to_device: true,
                synced_device_path: Some(PathBuf::from("/kindle/documents/SdrWarn.kfx")),
                sync_error: None,
                word_wise_glosses: Some(25),
                word_wise_error: None,
                sdr_error: Some("read-only volume".into()),
            }),
        );
        assert_eq!(dev_files.len(), 2);
        assert_eq!(dev_files[1].size_bytes, 0);
        assert_eq!(dev_files[1].size_formatted, "Synced");
        assert!(!dev_files[1].has_word_wise);
        assert!(status_msg.as_ref().is_some_and(|(m, is_err)| *is_err
            && m.contains("Sidecar (.sdr):")
            && m.contains("read-only volume")));
    }
}
