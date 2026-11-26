use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{BatchQueue, DropJob, KindleStudio};
use crate::state::{
    BatchProgressState, ConversionOutcome, ConversionResult, ConversionSettings, ConversionTask,
    DeviceStatus, EpubSortField, KindleFile, LocalEpubItem, LocalEpubStatus, ScanProgressState,
    SortDirection, StudioTab, TaskStatus, apply_conversion_outcome_to_state,
    collect_incoming_epub_paths, convert_epub_file, delete_device_file, detect_kindle_device,
    eject_kindle_device, format_file_size, format_task_error, load_device_files,
    resolve_output_folder_target, scan_local_epub_directory_with_progress,
};
use gpui::*;

const MAX_BATCH_WORKERS: usize = 2;
const MAX_DROP_WORKERS: usize = 4;
const MAX_TOTAL_CONVERSION_WORKERS: usize = 4;
const TAB_REFRESH_COOLDOWN: Duration = Duration::from_secs(3);

/// Resolves the final `ConversionOutcome` for an in-flight batch worker job.
///
/// If the worker already produced `Ok(result)` (having written/synced the KFX artifact),
/// preserve `ConversionOutcome::Finished(result)` even if `batch_cancel_token` was flipped
/// while the job was completing.
pub(super) fn resolve_batch_worker_outcome(
    cancelled: bool,
    res: Result<ConversionResult, String>,
) -> ConversionOutcome {
    match res {
        Ok(r) => ConversionOutcome::Finished(r),
        Err(_) if cancelled => ConversionOutcome::Cancelled,
        Err(err) => ConversionOutcome::Failed(err),
    }
}

pub(super) struct PreparedBatchRun {
    pub run_generation: u64,
    pub run_settings: ConversionSettings,
    pub device: DeviceStatus,
    pub cancel_token: Arc<AtomicBool>,
    pub queue: BatchQueue,
    pub num_workers: usize,
}

fn spawn_file_manager(dir: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg("--").arg(dir);
        c
    };
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("explorer");
        c.arg(dir);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(dir);
        c
    };

    cmd.spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to open output folder '{}': {}", dir.display(), e))
}

impl KindleStudio {
    pub(super) fn is_batch_active(&self) -> bool {
        let uncancelled_token = self
            .batch_cancel_token
            .as_ref()
            .is_some_and(|t| !t.load(Ordering::SeqCst));
        let active_uncancelled_progress =
            self.folder_state.batch_progress.as_ref().is_some_and(|bp| {
                !bp.is_cancelled
                    && (bp.active_tasks > 0
                        || bp.completed_tasks + bp.failed_tasks < bp.total_tasks)
            });
        self.batch_active_workers > 0 || uncancelled_token || active_uncancelled_progress
    }

    fn drop_queue_len(&self) -> usize {
        let guard = match self.drop_queue.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.len()
    }

    fn drop_queue_is_empty(&self) -> bool {
        self.drop_queue_len() == 0
    }

    pub(super) fn has_active_tasks(&self) -> bool {
        self.batch_active_workers > 0
            || self.drop_active_workers > 0
            || !self.drop_queue_is_empty()
            || self.delete_in_progress
            || self.tasks.iter().any(|t| {
                matches!(
                    t.status,
                    TaskStatus::Queued | TaskStatus::Converting | TaskStatus::Syncing
                )
            })
    }

    pub(super) fn apply_conversion_outcome(
        &mut self,
        task_id: u64,
        target_path: &Path,
        batch_generation: Option<u64>,
        outcome: ConversionOutcome,
    ) {
        if let ConversionOutcome::Finished(ref r) = outcome {
            self.last_output_path = Some(r.output_path.clone());
        }
        let bp_opt = match batch_generation {
            Some(batch_gen) if batch_gen == self.batch_generation => {
                self.folder_state.batch_progress.as_mut()
            }
            _ => None,
        };
        apply_conversion_outcome_to_state(
            &mut self.tasks,
            &mut self.folder_state.items,
            bp_opt,
            &mut self.files,
            &self.device,
            &mut self.status_message,
            task_id,
            target_path,
            outcome,
        );
        self.mark_folder_items_dirty();
        self.mark_files_dirty();
    }

    pub(super) fn apply_device_refresh_result(
        &mut self,
        generation: u64,
        device: DeviceStatus,
        mut files: Vec<KindleFile>,
        clear_status: bool,
    ) -> bool {
        if self.device_refresh_generation != generation {
            return false;
        }
        self.device_refresh_in_flight = false;
        if self.eject_in_progress {
            return false;
        }
        for file in &mut files {
            if file.author == "Unknown" {
                let file_stem = file
                    .full_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if let Some(item) = self.folder_state.items.iter().find(|i| {
                    i.path
                        .file_stem()
                        .is_some_and(|s| s.to_string_lossy() == file_stem)
                        || i.title.eq_ignore_ascii_case(&file.title)
                }) {
                    file.title = item.title.clone();
                    file.author = item.author.clone();
                }
            }
        }
        if let Some(ref pending_delete) = self.confirm_delete_file
            && !files.iter().any(|f| &f.full_path == pending_delete)
        {
            self.confirm_delete_file = None;
        }
        self.device = device;
        self.files = files;
        self.last_device_refresh_at = Some(Instant::now());
        self.mark_files_dirty();
        if clear_status {
            self.status_message = None;
        }
        true
    }

    pub(super) fn decide_tab_switch_refresh(
        &mut self,
        target_tab: StudioTab,
        now: Instant,
    ) -> bool {
        if self.tab == target_tab {
            return false;
        }
        self.tab = target_tab;
        if self.eject_in_progress || self.device_refresh_in_flight || self.has_active_tasks() {
            return false;
        }
        if self
            .last_device_refresh_at
            .is_some_and(|last| now.saturating_duration_since(last) < TAB_REFRESH_COOLDOWN)
        {
            return false;
        }
        true
    }

    pub(super) fn switch_tab(&mut self, target_tab: StudioTab, cx: &mut Context<Self>) {
        let should_refresh = self.decide_tab_switch_refresh(target_tab, Instant::now());
        cx.notify();
        if should_refresh {
            self.refresh_device_internal(false, cx);
        }
    }

    pub fn refresh_device(&mut self, cx: &mut Context<Self>) {
        self.refresh_device_internal(true, cx);
    }

    pub(super) fn refresh_device_internal(&mut self, clear_status: bool, cx: &mut Context<Self>) {
        if self.eject_in_progress {
            return;
        }
        if !clear_status && self.device_refresh_in_flight {
            return;
        }
        self.device_refresh_generation = self.device_refresh_generation.wrapping_add(1);
        let generation = self.device_refresh_generation;
        self.device_refresh_in_flight = true;

        cx.spawn(async move |this, cx| {
            let (device, files) = cx
                .background_executor()
                .spawn(async move {
                    let dev = detect_kindle_device();
                    let f = load_device_files(&dev);
                    (dev, f)
                })
                .await;

            this.update(cx, |this, cx| {
                if this.apply_device_refresh_result(generation, device, files, clear_status) {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn try_begin_eject(&mut self) -> Result<DeviceStatus, String> {
        if !self.device.connected {
            return Err("No Kindle device connected to eject.".into());
        }
        if self.eject_in_progress {
            return Err("Kindle eject is already in progress.".into());
        }
        if self.has_active_tasks() {
            return Err(
                "Cannot eject Kindle while conversion, sync, or delete tasks are active.".into(),
            );
        }

        let device = self.device.clone();
        self.eject_in_progress = true;
        self.confirm_delete_file = None;
        self.device_refresh_generation = self.device_refresh_generation.wrapping_add(1);
        self.device_refresh_in_flight = false;
        self.status_message = Some(("Ejecting Kindle device...".into(), false));
        Ok(device)
    }

    pub fn eject_device(&mut self, cx: &mut Context<Self>) {
        let device = match self.try_begin_eject() {
            Ok(d) => d,
            Err(msg) => {
                self.status_message = Some((msg, true));
                cx.notify();
                return;
            }
        };
        cx.notify();

        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { eject_kindle_device(&device) })
                .await;

            this.update(cx, |this, cx| {
                this.eject_in_progress = false;
                this.device_refresh_generation = this.device_refresh_generation.wrapping_add(1);
                match res {
                    Ok(_) => {
                        this.status_message = Some(("Kindle device safely ejected.".into(), false));
                        this.device = DeviceStatus::default();
                        this.files.clear();
                        this.mark_files_dirty();
                        this.confirm_delete_file = None;
                    }
                    Err(err) => {
                        this.status_message = Some((format_task_error("Eject failed", &err), true));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn try_begin_delete_file(
        &mut self,
        full_path: &Path,
    ) -> Result<(KindleFile, PathBuf), (String, bool)> {
        if self.eject_in_progress {
            self.confirm_delete_file = None;
            return Err((
                "Cannot delete book while Kindle eject is in progress.".into(),
                true,
            ));
        }
        if !self.device.connected {
            self.confirm_delete_file = None;
            return Err((
                "Cannot delete book: no Kindle device connected.".into(),
                true,
            ));
        }
        if self.delete_in_progress {
            return Err(("A file deletion is already in progress.".into(), true));
        }

        let Some(pos) = self.files.iter().position(|f| f.full_path == full_path) else {
            self.confirm_delete_file = None;
            return Err((
                "Book is no longer present on device; delete cancelled safely.".into(),
                false,
            ));
        };

        let file = self.files[pos].clone();
        let Some(allowed_root) = self
            .device
            .path
            .clone()
            .or_else(|| self.device.root_path.clone())
        else {
            self.confirm_delete_file = None;
            return Err((
                "Cannot delete file: Kindle storage root is not available.".into(),
                true,
            ));
        };

        self.confirm_delete_file = None;
        self.delete_in_progress = true;
        self.status_message = Some((format!("Deleting '{}'...", file.name), false));
        Ok((file, allowed_root))
    }

    pub fn delete_file(&mut self, full_path: &Path, cx: &mut Context<Self>) {
        let (file, allowed_root) = match self.try_begin_delete_file(full_path) {
            Ok(pair) => pair,
            Err((msg, is_err)) => {
                self.status_message = Some((msg, is_err));
                cx.notify();
                return;
            }
        };

        let target_path = file.full_path.clone();
        let display_name = file.name.clone();
        cx.notify();

        cx.spawn(async move |this, cx| {
            let (del_res, new_device) = cx
                .background_executor()
                .spawn(async move {
                    let res = delete_device_file(Some(&allowed_root), &file);
                    let dev = detect_kindle_device();
                    (res, dev)
                })
                .await;

            this.update(cx, |this, cx| {
                this.delete_in_progress = false;
                match del_res {
                    Ok(_) => {
                        this.files.retain(|f| f.full_path != target_path);
                        this.mark_files_dirty();
                        this.status_message =
                            Some((format!("'{}' book and cache deleted.", display_name), false));
                        if !this.eject_in_progress && new_device.connected {
                            this.device = new_device;
                        }
                    }
                    Err(err) => {
                        this.status_message = Some((format_task_error("Delete", &err), true));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn select_folder(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select folder containing EPUB books".into()),
        });

        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await
                && let Some(folder) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| {
                    this.folder_state.current_folder = Some(folder);
                    this.rescan_folder(cx);
                })
                .ok();
            }
        })
        .detach();
    }

    pub fn cancel_folder_scan(&mut self, cx: &mut Context<Self>) {
        if let Some(token) = self.scan_cancel_token.take() {
            token.store(true, Ordering::Release);
        }
        self.folder_state.scan_generation = self.folder_state.scan_generation.wrapping_add(1);
        self.folder_state.is_scanning = false;
        self.folder_state.scan_progress = None;
        cx.notify();
    }

    pub(super) fn has_active_folder_conversions(&self) -> bool {
        self.folder_state.items.iter().any(|i| {
            matches!(
                i.status,
                LocalEpubStatus::InQueue | LocalEpubStatus::Processing { .. }
            )
        })
    }

    pub(super) fn can_start_folder_rescan(&self) -> Result<(), String> {
        if self.is_batch_active() || self.has_active_folder_conversions() {
            return Err("Cannot refresh folder while conversion is in progress.".into());
        }
        Ok(())
    }

    pub(super) fn apply_folder_scan_result(
        &mut self,
        generation: u64,
        items: Vec<LocalEpubItem>,
    ) -> bool {
        if self.folder_state.scan_generation != generation {
            return false;
        }
        self.folder_state.is_scanning = false;
        self.folder_state.scan_progress = None;
        self.scan_cancel_token = None;
        if self.is_batch_active() || self.has_active_folder_conversions() {
            return false;
        }
        self.folder_state.items = items;
        self.mark_folder_items_dirty();
        true
    }

    pub fn rescan_folder(&mut self, cx: &mut Context<Self>) {
        if let Err(msg) = self.can_start_folder_rescan() {
            self.status_message = Some((msg, true));
            cx.notify();
            return;
        }
        let Some(folder) = self.folder_state.current_folder.clone() else {
            return;
        };

        if let Some(prev_token) = self.scan_cancel_token.take() {
            prev_token.store(true, Ordering::Release);
        }

        self.folder_state.scan_generation = self.folder_state.scan_generation.wrapping_add(1);
        let generation = self.folder_state.scan_generation;

        let cancel_token = Arc::new(AtomicBool::new(false));
        self.scan_cancel_token = Some(cancel_token.clone());

        self.folder_state.is_scanning = true;
        self.folder_state.scan_progress = Some(ScanProgressState::discovering());
        cx.notify();

        let recursive = self.folder_state.recursive_scan;
        let device = self.device.clone();

        let shared_progress = Arc::new(Mutex::new(ScanProgressState::discovering()));
        let result_slot: Arc<Mutex<Option<Vec<LocalEpubItem>>>> = Arc::new(Mutex::new(None));
        let done = Arc::new(AtomicBool::new(false));

        let progress_bg = shared_progress.clone();
        let result_bg = result_slot.clone();
        let done_bg = done.clone();
        let cancel_bg = cancel_token.clone();

        cx.background_executor()
            .spawn(async move {
                let items = scan_local_epub_directory_with_progress(
                    &folder,
                    recursive,
                    &device,
                    Some(&progress_bg),
                    Some(&cancel_bg),
                );
                if let Ok(mut guard) = result_bg.lock() {
                    *guard = Some(items);
                }
                done_bg.store(true, Ordering::Release);
            })
            .detach();

        cx.spawn(async move |this, cx| {
            while !done.load(Ordering::Acquire) {
                cx.background_executor()
                    .timer(Duration::from_millis(80))
                    .await;

                if cancel_token.load(Ordering::Acquire) {
                    return;
                }

                let snapshot = match shared_progress.lock() {
                    Ok(g) => g.clone(),
                    Err(p) => p.into_inner().clone(),
                };

                let mut still_active = true;
                this.update(cx, |this, cx| {
                    if this.folder_state.scan_generation != generation {
                        still_active = false;
                        return;
                    }
                    this.folder_state.scan_progress = Some(snapshot);
                    cx.notify();
                })
                .ok();

                if !still_active {
                    return;
                }
            }

            if cancel_token.load(Ordering::Acquire) {
                return;
            }

            let items = match result_slot.lock() {
                Ok(mut g) => g.take().unwrap_or_default(),
                Err(p) => p.into_inner().take().unwrap_or_default(),
            };

            this.update(cx, |this, cx| {
                if this.folder_state.scan_generation != generation {
                    return;
                }
                this.apply_folder_scan_result(generation, items);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn toggle_select_all(&mut self, select: bool, cx: &mut Context<Self>) {
        for item in &mut self.folder_state.items {
            item.is_selected = select;
        }
        self.mark_folder_items_dirty();
        cx.notify();
    }

    pub fn select_unconverted_only(&mut self, cx: &mut Context<Self>) {
        for item in &mut self.folder_state.items {
            item.is_selected = item.status == LocalEpubStatus::Unconverted;
        }
        self.mark_folder_items_dirty();
        cx.notify();
    }

    pub fn toggle_item_selection(&mut self, item_id: u64, cx: &mut Context<Self>) {
        if let Some(item) = self.folder_state.items.iter_mut().find(|i| i.id == item_id) {
            item.is_selected = !item.is_selected;
            self.mark_folder_items_dirty();
        }
        cx.notify();
    }

    pub(super) fn cancel_batch_state(&mut self) {
        if let Some(ref token) = self.batch_cancel_token {
            token.store(true, Ordering::SeqCst);
        }
        let active_gen = self.batch_generation;

        let drained: Vec<(u64, PathBuf)> = if let Some(ref queue) = self.batch_queue {
            let mut guard = match queue.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.drain(..).collect()
        } else {
            Vec::new()
        };

        for (task_id, path_buf) in drained {
            self.apply_conversion_outcome(
                task_id,
                &path_buf,
                Some(active_gen),
                ConversionOutcome::Cancelled,
            );
        }

        let drop_task_ids: std::collections::HashSet<u64> = {
            let guard = match self.drop_queue.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.iter().map(|j| j.task_id).collect()
        };
        let remaining_queued_ids: Vec<u64> = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Queued && !drop_task_ids.contains(&t.id))
            .map(|t| t.id)
            .collect();
        for task_id in remaining_queued_ids {
            self.apply_conversion_outcome(
                task_id,
                Path::new(""),
                Some(active_gen),
                ConversionOutcome::Cancelled,
            );
        }

        for item in self.folder_state.items.iter_mut() {
            if item.status == LocalEpubStatus::InQueue {
                item.status = LocalEpubStatus::Unconverted;
            }
        }
        self.mark_folder_items_dirty();
        if let Some(ref mut bp) = self.folder_state.batch_progress {
            bp.is_cancelled = true;
            bp.current_stage = "Cancelled remaining queue.".into();
        }
        if self.batch_active_workers == 0 {
            self.batch_cancel_token = None;
            self.batch_queue = None;
        }
        self.status_message = Some(("Batch operation cancelled.".into(), false));
    }

    pub fn cancel_batch_conversion(&mut self, cx: &mut Context<Self>) {
        self.cancel_batch_state();
        cx.notify();
    }

    pub(super) fn try_prepare_batch_run(
        &mut self,
        sync_to_kindle: bool,
    ) -> Result<PreparedBatchRun, String> {
        if self.eject_in_progress {
            return Err("Cannot start batch conversion while Kindle eject is in progress.".into());
        }
        if self.is_batch_active() {
            return Err("A batch conversion is already in progress.".into());
        }
        if self.drop_active_workers > 0 || !self.drop_queue_is_empty() {
            return Err(
                "Cannot start batch conversion while drag-and-drop conversions are active.".into(),
            );
        }
        if sync_to_kindle && !self.device.connected {
            return Err("Cannot transfer to Kindle: no Kindle device connected.".into());
        }

        let selected_items: Vec<(PathBuf, String)> = self
            .folder_state
            .items
            .iter()
            .filter(|i| i.is_selected)
            .map(|i| (i.path.clone(), i.file_size_str.clone()))
            .collect();

        if selected_items.is_empty() {
            return Err("Please select books to convert first.".into());
        }

        // Signal cancel on any stale prior token before bumping batch generation.
        if let Some(prev_token) = self.batch_cancel_token.take() {
            prev_token.store(true, Ordering::SeqCst);
        }
        self.batch_generation = self.batch_generation.wrapping_add(1);
        let run_generation = self.batch_generation;

        let count = selected_items.len();
        self.status_message = Some((
            format!(
                "Starting bounded conversion of {} books (2 concurrent workers)...",
                count
            ),
            false,
        ));

        let mut run_settings = self.settings.clone();
        run_settings.auto_deploy = sync_to_kindle;

        let cancel_token = Arc::new(AtomicBool::new(false));
        self.batch_cancel_token = Some(cancel_token.clone());

        self.folder_state.batch_progress = Some(BatchProgressState {
            total_tasks: count,
            completed_tasks: 0,
            failed_tasks: 0,
            active_tasks: 0,
            current_filename: String::new(),
            current_stage: if sync_to_kindle {
                "Preparing transfer...".into()
            } else {
                "Preparing conversion...".into()
            },
            is_sync: sync_to_kindle,
            is_cancelled: false,
        });

        for item in self.folder_state.items.iter_mut() {
            if item.is_selected {
                item.status = LocalEpubStatus::InQueue;
            }
        }
        self.mark_folder_items_dirty();

        let mut queued_tasks = VecDeque::with_capacity(count);
        for (path, file_size_str) in &selected_items {
            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let task_id = self.allocate_task_id();
            queued_tasks.push_back((task_id, path.clone()));
            self.tasks.insert(
                0,
                ConversionTask {
                    id: task_id,
                    filename,
                    file_size_str: file_size_str.clone(),
                    status: TaskStatus::Queued,
                    progress: 0.0,
                    word_wise: run_settings.word_wise,
                },
            );
        }

        let device = self.device.clone();

        // Bounded worker pool: max 2 concurrent batch workers within global 4-worker ceiling
        let queue = Arc::new(Mutex::new(queued_tasks));
        self.batch_queue = Some(queue.clone());
        let available_budget =
            MAX_TOTAL_CONVERSION_WORKERS.saturating_sub(self.drop_active_workers);
        let num_workers = MAX_BATCH_WORKERS.min(available_budget).min(count);
        self.batch_active_workers = num_workers;

        Ok(PreparedBatchRun {
            run_generation,
            run_settings,
            device,
            cancel_token,
            queue,
            num_workers,
        })
    }

    pub fn convert_selected_epubs(&mut self, sync_to_kindle: bool, cx: &mut Context<Self>) {
        let PreparedBatchRun {
            run_generation,
            run_settings,
            device,
            cancel_token,
            queue,
            num_workers,
        } = match self.try_prepare_batch_run(sync_to_kindle) {
            Ok(ok) => ok,
            Err(msg) => {
                self.status_message = Some((msg, true));
                cx.notify();
                return;
            }
        };
        cx.notify();

        for _ in 0..num_workers {
            let q = queue.clone();
            let s_clone = run_settings.clone();
            let d_clone = device.clone();
            let cancel = cancel_token.clone();

            cx.spawn(async move |this, cx| {
                loop {
                    if cancel.load(Ordering::SeqCst) {
                        break;
                    }

                    let next = {
                        let mut guard = match q.lock() {
                            Ok(g) => g,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        guard.pop_front()
                    };

                    let Some((task_id, path_buf)) = next else {
                        break;
                    };

                    let filename = path_buf
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    let mut should_run = false;
                    let _ = this.update(cx, |this, cx| {
                        if cancel.load(Ordering::SeqCst) || this.batch_generation != run_generation
                        {
                            this.apply_conversion_outcome(
                                task_id,
                                &path_buf,
                                Some(run_generation),
                                ConversionOutcome::Cancelled,
                            );
                            cx.notify();
                            return;
                        }
                        should_run = true;
                        if let Some(t) = this.tasks.iter_mut().find(|t| t.id == task_id) {
                            t.status = TaskStatus::Converting;
                            t.progress = 0.3;
                        }
                        if let Some(ref mut bp) = this.folder_state.batch_progress {
                            bp.active_tasks += 1;
                            bp.current_filename = filename.clone();
                            bp.current_stage = if s_clone.auto_deploy {
                                "Packaging & Transferring...".into()
                            } else {
                                "Packaging KFX...".into()
                            };
                        }
                        if let Some(item) = this
                            .folder_state
                            .items
                            .iter_mut()
                            .find(|i| i.path == path_buf)
                        {
                            item.status = LocalEpubStatus::Processing {
                                stage: if s_clone.auto_deploy {
                                    "Packaging & Sync".into()
                                } else {
                                    "Packaging KFX".into()
                                },
                                progress: 30,
                            };
                            this.mark_folder_items_dirty();
                        }
                        cx.notify();
                    });

                    if !should_run {
                        break;
                    }

                    let target_path = path_buf.clone();
                    let s = s_clone.clone();
                    let d = d_clone.clone();
                    let res = cx
                        .background_executor()
                        .spawn(async move {
                            convert_epub_file(&target_path, &s, &d, None::<fn(f32, &str)>)
                        })
                        .await;

                    let cancel_check = cancel.clone();
                    let _ = this.update(cx, |this, cx| {
                        if this.batch_generation == run_generation
                            && let Some(ref mut bp) = this.folder_state.batch_progress
                        {
                            bp.active_tasks = bp.active_tasks.saturating_sub(1);
                        }
                        let outcome =
                            resolve_batch_worker_outcome(cancel_check.load(Ordering::SeqCst), res);
                        this.apply_conversion_outcome(
                            task_id,
                            &path_buf,
                            Some(run_generation),
                            outcome,
                        );
                        cx.notify();
                    });
                }

                let _ = this.update(cx, |this, cx| {
                    if this.batch_generation == run_generation {
                        this.batch_active_workers = this.batch_active_workers.saturating_sub(1);
                        if this.batch_active_workers == 0 {
                            this.batch_cancel_token = None;
                            this.batch_queue = None;
                        }
                        this.maybe_wake_drop_workers(cx);
                        cx.notify();
                    }
                });
            })
            .detach();
        }
    }

    pub(super) fn try_prepare_single_convert(
        &mut self,
        path: &Path,
        sync_to_kindle: bool,
    ) -> Result<(ConversionSettings, Option<String>), String> {
        if self.eject_in_progress {
            return Err("Cannot convert book while Kindle eject is in progress.".into());
        }
        if sync_to_kindle && !self.device.connected {
            return Err("Cannot transfer to Kindle: no Kindle device connected.".into());
        }
        if self.is_batch_active() {
            return Err("Cannot start single conversion while batch conversion is active.".into());
        }
        if self.folder_state.items.iter().any(|i| {
            i.path == path
                && matches!(
                    i.status,
                    LocalEpubStatus::InQueue | LocalEpubStatus::Processing { .. }
                )
        }) {
            return Err("Book conversion is already in progress.".into());
        }

        let mut size_opt = None;
        if let Some(item) = self.folder_state.items.iter_mut().find(|i| i.path == path) {
            size_opt = Some(item.file_size_str.clone());
            item.status = LocalEpubStatus::Processing {
                stage: if sync_to_kindle {
                    "Packaging & Sync".into()
                } else {
                    "Packaging KFX".into()
                },
                progress: 30,
            };
            self.mark_folder_items_dirty();
        }

        let mut run_settings = self.settings.clone();
        run_settings.auto_deploy = sync_to_kindle;
        Ok((run_settings, size_opt))
    }

    pub fn convert_single_epub(
        &mut self,
        path: PathBuf,
        sync_to_kindle: bool,
        cx: &mut Context<Self>,
    ) {
        let (run_settings, size_opt) = match self.try_prepare_single_convert(&path, sync_to_kindle)
        {
            Ok(ok) => ok,
            Err(msg) => {
                self.status_message = Some((msg, true));
                cx.notify();
                return;
            }
        };
        self.enqueue_drop_epub_jobs(vec![(path, size_opt)], run_settings, cx);
    }

    pub fn handle_incoming_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.eject_in_progress {
            self.status_message = Some((
                "Cannot accept EPUBs while Kindle eject is in progress.".into(),
                true,
            ));
            cx.notify();
            return;
        }
        let incoming = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let discovered = cx
                .background_executor()
                .spawn(async move { collect_incoming_epub_paths(&incoming) })
                .await;

            if discovered.is_empty() {
                return;
            }
            this.update(cx, |this, cx| {
                let jobs = discovered
                    .into_iter()
                    .map(|(p, sz)| (p, Some(sz)))
                    .collect();
                let settings = this.settings.clone();
                this.enqueue_drop_epub_jobs(jobs, settings, cx);
            })
            .ok();
        })
        .detach();
    }

    #[allow(dead_code)]
    pub fn process_epub_file(&mut self, path: &Path, cx: &mut Context<Self>) {
        if self.eject_in_progress {
            self.status_message = Some((
                "Cannot convert book while Kindle eject is in progress.".into(),
                true,
            ));
            cx.notify();
            return;
        }
        let settings = self.settings.clone();
        self.enqueue_drop_epub_jobs(vec![(path.to_path_buf(), None)], settings, cx);
    }

    pub(super) fn enqueue_drop_jobs_state(
        &mut self,
        items: Vec<(PathBuf, Option<String>)>,
        run_settings: ConversionSettings,
    ) -> Result<usize, String> {
        if items.is_empty() {
            return Ok(0);
        }
        if self.eject_in_progress {
            return Err("Cannot start conversion while Kindle eject is in progress.".into());
        }

        let mut prepared = Vec::with_capacity(items.len());
        for (path, size_opt) in items {
            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let file_size_str = size_opt.unwrap_or_else(|| "Pending...".into());
            let task_id = self.allocate_task_id();
            self.tasks.insert(
                0,
                ConversionTask {
                    id: task_id,
                    filename,
                    file_size_str,
                    status: TaskStatus::Queued,
                    progress: 0.0,
                    word_wise: run_settings.word_wise,
                },
            );
            prepared.push(DropJob {
                task_id,
                path,
                settings: run_settings.clone(),
            });
        }

        let queue_len = {
            let queue = self.drop_queue.clone();
            let mut guard = match queue.lock() {
                Ok(g) => g,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.extend(prepared);
            guard.len()
        };

        let to_spawn = self.compute_drop_workers_to_spawn(queue_len);
        self.drop_active_workers += to_spawn;
        Ok(to_spawn)
    }

    pub(super) fn compute_drop_workers_to_spawn(&self, queue_len: usize) -> usize {
        if queue_len == 0 {
            return 0;
        }
        let desired_drop = MAX_DROP_WORKERS.min(self.drop_active_workers.saturating_add(queue_len));
        let budget_for_drop =
            MAX_TOTAL_CONVERSION_WORKERS.saturating_sub(self.batch_active_workers);
        let target_workers = desired_drop.min(budget_for_drop);
        target_workers.saturating_sub(self.drop_active_workers)
    }

    pub(super) fn claim_pending_drop_workers_state(&mut self) -> usize {
        let queue_len = self.drop_queue_len();
        let to_spawn = self.compute_drop_workers_to_spawn(queue_len);
        self.drop_active_workers += to_spawn;
        to_spawn
    }

    fn maybe_wake_drop_workers(&mut self, cx: &mut Context<Self>) {
        let to_spawn = self.claim_pending_drop_workers_state();
        if to_spawn > 0 {
            self.spawn_drop_workers(to_spawn, cx);
        }
    }

    fn enqueue_drop_epub_jobs(
        &mut self,
        items: Vec<(PathBuf, Option<String>)>,
        run_settings: ConversionSettings,
        cx: &mut Context<Self>,
    ) {
        let had_items = !items.is_empty();
        let to_spawn = match self.enqueue_drop_jobs_state(items, run_settings) {
            Ok(n) => n,
            Err(msg) => {
                self.status_message = Some((msg, true));
                cx.notify();
                return;
            }
        };
        if had_items {
            cx.notify();
        }
        if to_spawn == 0 {
            return;
        }
        self.spawn_drop_workers(to_spawn, cx);
    }

    pub(super) fn should_apply_drop_detected_device(
        &self,
        detected: &DeviceStatus,
        refresh_gen: u64,
    ) -> bool {
        detected.connected
            && !self.eject_in_progress
            && self.device_refresh_generation == refresh_gen
            && !self.device.connected
    }

    fn spawn_drop_workers(&mut self, count: usize, cx: &mut Context<Self>) {
        for _ in 0..count {
            let q = self.drop_queue.clone();
            cx.spawn(async move |this, cx| {
                loop {
                    let next_job = {
                        let mut guard = match q.lock() {
                            Ok(g) => g,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        guard.pop_front()
                    };

                    let Some(job) = next_job else {
                        break;
                    };

                    let mut current_device = DeviceStatus::default();
                    let mut refresh_gen = 0u64;
                    let _ = this.update(cx, |this, cx| {
                        current_device = this.device.clone();
                        refresh_gen = this.device_refresh_generation;
                        if let Some(t) = this.tasks.iter_mut().find(|t| t.id == job.task_id) {
                            t.status = TaskStatus::Converting;
                            t.progress = 0.5;
                        }
                        cx.notify();
                    });

                    let bg_path = job.path.clone();
                    let bg_settings = job.settings.clone();
                    let (size_str_opt, detected_device, res) = cx
                        .background_executor()
                        .spawn(async move {
                            let size_opt = std::fs::metadata(&bg_path)
                                .ok()
                                .map(|m| format_file_size(m.len()));
                            let detected = if bg_settings.auto_deploy && !current_device.connected {
                                Some(detect_kindle_device())
                            } else {
                                None
                            };
                            let effective_device = detected.as_ref().unwrap_or(&current_device);
                            let conv_res = convert_epub_file(
                                &bg_path,
                                &bg_settings,
                                effective_device,
                                None::<fn(f32, &str)>,
                            );
                            (size_opt, detected, conv_res)
                        })
                        .await;

                    let _ = this.update(cx, |this, cx| {
                        if let Some(size_str) = size_str_opt
                            && let Some(t) = this.tasks.iter_mut().find(|t| t.id == job.task_id)
                            && t.file_size_str == "Pending..."
                        {
                            t.file_size_str = size_str;
                        }
                        let mut needs_device_file_refresh = false;
                        if let Some(dev) = detected_device
                            && this.should_apply_drop_detected_device(&dev, refresh_gen)
                        {
                            this.device = dev;
                            needs_device_file_refresh = true;
                        }
                        let outcome = match res {
                            Ok(r) => ConversionOutcome::Finished(r),
                            Err(err) => ConversionOutcome::Failed(err),
                        };
                        this.apply_conversion_outcome(job.task_id, &job.path, None, outcome);
                        if needs_device_file_refresh {
                            this.refresh_device_internal(false, cx);
                        }
                        cx.notify();
                    });
                }

                let _ = this.update(cx, |this, cx| {
                    this.drop_active_workers = this.drop_active_workers.saturating_sub(1);
                    this.maybe_wake_drop_workers(cx);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    pub fn open_output_folder(&mut self, target: &Path, cx: &mut Context<Self>) {
        let target_buf = target.to_path_buf();
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move {
                    let dir = resolve_output_folder_target(&target_buf)?;
                    spawn_file_manager(&dir)
                })
                .await;

            if let Err(err) = res {
                this.update(cx, |this, cx| {
                    this.status_message = Some((err, true));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub(super) fn apply_sort_column_transition(
        sort_field: &mut EpubSortField,
        sort_direction: &mut SortDirection,
        clicked_field: EpubSortField,
    ) {
        if *sort_field == clicked_field {
            *sort_direction = sort_direction.toggle();
        } else {
            *sort_field = clicked_field;
            *sort_direction = match clicked_field {
                EpubSortField::DateModified | EpubSortField::FileSize => SortDirection::Descending,
                _ => SortDirection::Ascending,
            };
        }
    }

    pub(super) fn handle_sort_column_click(
        &mut self,
        field: EpubSortField,
        cx: &mut Context<Self>,
    ) {
        Self::apply_sort_column_transition(
            &mut self.folder_state.sort_field,
            &mut self.folder_state.sort_direction,
            field,
        );
        cx.notify();
    }

    pub(super) fn handle_explorer_sort_column_click(
        &mut self,
        field: EpubSortField,
        cx: &mut Context<Self>,
    ) {
        Self::apply_sort_column_transition(
            &mut self.explorer_sort_field,
            &mut self.explorer_sort_direction,
            field,
        );
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::super::ExplorerFilterCounts;
    use super::KindleStudio;
    use crate::state::{
        ConversionOutcome, ConversionResult, ConversionSettings, ConversionTask, DeviceStatus,
        EpubSortField, KindleFile, LocalEpubItem, LocalEpubStatus, SortDirection, TaskStatus,
        collect_incoming_epub_paths,
    };
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    fn dummy_item(id: u64, path: PathBuf, selected: bool) -> LocalEpubItem {
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        LocalEpubItem {
            id,
            path,
            filename: filename.clone(),
            title: filename,
            author: "Author".into(),
            file_size_bytes: 1024,
            file_size_str: "1 KB".into(),
            modified_timestamp: 0,
            modified_str: "Just now".into(),
            is_selected: selected,
            status: LocalEpubStatus::Unconverted,
        }
    }

    #[test]
    fn test_auto_deploy_not_mutated_and_monotonic_task_ids() {
        let mut studio = KindleStudio::new_for_test();
        studio.settings.auto_deploy = false;
        studio.device = DeviceStatus {
            connected: true,
            model: "Kindle".into(),
            path: Some(PathBuf::from("/tmp/docs")),
            root_path: Some(PathBuf::from("/tmp")),
            used_gb: 1.0,
            total_gb: 8.0,
        };
        let epub1 = PathBuf::from("/books/One.epub");
        let epub2 = PathBuf::from("/books/Two.epub");
        studio.folder_state.items = vec![
            dummy_item(1, epub1.clone(), true),
            dummy_item(2, epub2.clone(), false),
        ];

        let prep1 = studio
            .try_prepare_batch_run(true)
            .expect("Batch run should prepare");
        assert_eq!(prep1.num_workers, 1);
        assert!(prep1.run_settings.auto_deploy);
        assert!(
            !studio.settings.auto_deploy,
            "Batch sync run must NOT mutate self.settings.auto_deploy"
        );
        let id1 = studio.tasks[0].id;
        assert_eq!(id1, 1);

        // Simulate completion and clearing completed tasks, then run single convert with sync=false
        studio.apply_conversion_outcome(
            id1,
            &epub1,
            Some(prep1.run_generation),
            ConversionOutcome::Failed("mock".into()),
        );
        studio.batch_active_workers = 0;
        studio.batch_cancel_token = None;
        studio.batch_queue = None;
        studio.tasks.clear();

        studio.settings.auto_deploy = true;
        let (single_settings, _) = studio
            .try_prepare_single_convert(&epub2, false)
            .expect("Single convert should prepare");
        assert!(!single_settings.auto_deploy);
        assert!(
            studio.settings.auto_deploy,
            "Single convert(sync=false) must NOT mutate self.settings.auto_deploy"
        );

        let spawned = studio
            .enqueue_drop_jobs_state(vec![(epub2, None)], single_settings)
            .expect("Drop enqueue should succeed");
        assert_eq!(spawned, 1);
        let id2 = studio.tasks[0].id;
        assert!(
            id2 > id1,
            "next_task_id must stay strictly monotonic even after clearing tasks ({id2} > {id1})"
        );
    }

    #[test]
    fn test_entry_guards_disconnected_empty_and_busy_eject() {
        let mut studio = KindleStudio::new_for_test();

        // 1. Disconnected device rejects sync batch, sync single, eject, and delete
        studio.folder_state.items = vec![dummy_item(1, PathBuf::from("/tmp/A.epub"), true)];
        assert!(studio.try_prepare_batch_run(true).is_err());
        assert!(
            studio
                .try_prepare_single_convert(Path::new("/tmp/A.epub"), true)
                .is_err()
        );
        assert!(studio.try_begin_eject().is_err());
        assert!(
            studio
                .try_begin_delete_file(Path::new("/tmp/A.kfx"))
                .is_err()
        );

        // 2. Empty selection rejected
        studio.folder_state.items[0].is_selected = false;
        assert!(studio.try_prepare_batch_run(false).is_err());

        // 3. Active task prevents eject_device
        studio.device = DeviceStatus {
            connected: true,
            model: "Kindle".into(),
            path: Some(PathBuf::from("/tmp")),
            root_path: Some(PathBuf::from("/tmp")),
            used_gb: 1.0,
            total_gb: 8.0,
        };
        studio.tasks.push(ConversionTask {
            id: 42,
            filename: "Active.epub".into(),
            file_size_str: "1 KB".into(),
            status: TaskStatus::Converting,
            progress: 0.5,
            word_wise: true,
        });
        assert!(studio.try_begin_eject().is_err());

        // 4. Active eject prevents batch, single, drop enqueue, delete, and stale device refresh
        studio.tasks.clear();
        studio.eject_in_progress = true;
        studio.folder_state.items[0].is_selected = true;
        assert!(studio.try_prepare_batch_run(false).is_err());
        assert!(
            studio
                .try_prepare_single_convert(Path::new("/tmp/A.epub"), false)
                .is_err()
        );
        assert!(
            studio
                .enqueue_drop_jobs_state(
                    vec![(PathBuf::from("/tmp/A.epub"), None)],
                    ConversionSettings::default()
                )
                .is_err()
        );
        assert!(
            studio
                .try_begin_delete_file(Path::new("/tmp/A.kfx"))
                .is_err()
        );

        let stale_gen = studio.device_refresh_generation;
        assert!(!studio.apply_device_refresh_result(
            stale_gen,
            DeviceStatus {
                connected: true,
                model: "Kindle".into(),
                path: Some(PathBuf::from("/tmp")),
                root_path: Some(PathBuf::from("/tmp")),
                used_gb: 1.0,
                total_gb: 8.0,
            },
            Vec::new(),
            false
        ));
    }

    #[test]
    fn test_delete_file_pathbuf_keyed_survives_concurrent_refresh() {
        let mut studio = KindleStudio::new_for_test();
        let author_a = PathBuf::from("/kindle/documents/AuthorA/Dune.kfx");
        let author_b = PathBuf::from("/kindle/documents/AuthorB/Dune.kfx");
        let make_file = |p: &PathBuf, author: &str| KindleFile {
            name: "Dune.kfx".into(),
            title: "Dune".into(),
            author: author.into(),
            format: "KFX".into(),
            size_formatted: "1 KB".into(),
            size_bytes: 100,
            modified: "Just now".into(),
            modified_timestamp: 0,
            full_path: p.clone(),
            has_word_wise: false,
            has_sdr: false,
        };

        studio.device = DeviceStatus {
            connected: true,
            model: "Kindle".into(),
            path: Some(PathBuf::from("/kindle/documents")),
            root_path: Some(PathBuf::from("/kindle")),
            used_gb: 1.0,
            total_gb: 8.0,
        };

        // Concurrent refresh removed AuthorA/Dune.kfx while AuthorB/Dune.kfx (same filename) remains
        studio.files = vec![make_file(&author_b, "AuthorB")];
        studio.confirm_delete_file = Some(author_a.clone());
        let res = studio.try_begin_delete_file(&author_a);
        assert!(res.is_err());
        assert!(studio.confirm_delete_file.is_none());
        assert_eq!(studio.files.len(), 1);
        assert_eq!(studio.files[0].full_path, author_b);

        // When both exist, targeting AuthorA/Dune.kfx resolves ONLY AuthorA/Dune.kfx
        studio.files = vec![
            make_file(&author_a, "AuthorA"),
            make_file(&author_b, "AuthorB"),
        ];
        studio.confirm_delete_file = Some(author_a.clone());
        let (target, _) = studio
            .try_begin_delete_file(&author_a)
            .expect("Exact PathBuf match should succeed");
        assert_eq!(target.full_path, author_a);
        assert!(studio.confirm_delete_file.is_none());
    }

    #[test]
    fn test_batch_cancel_cleans_queued_tasks_and_prior_token() {
        let mut studio = KindleStudio::new_for_test();
        studio.folder_state.items = (1..=4)
            .map(|i| dummy_item(i, PathBuf::from(format!("/books/Book{i}.epub")), true))
            .collect();

        let prep1 = studio
            .try_prepare_batch_run(false)
            .expect("First batch prepares");
        assert_eq!(prep1.run_generation, 1);
        assert!(!prep1.cancel_token.load(Ordering::SeqCst));

        // Simulate active worker count finishing at cancel
        studio.batch_active_workers = 0;
        studio.cancel_batch_state();

        assert!(prep1.cancel_token.load(Ordering::SeqCst));
        assert!(studio.batch_cancel_token.is_none());
        assert!(studio.batch_queue.is_none());
        assert!(
            !studio
                .tasks
                .iter()
                .any(|t| t.status == TaskStatus::Queued || t.status == TaskStatus::Converting)
        );
        assert!(
            !studio
                .folder_state
                .items
                .iter()
                .any(|i| i.status == LocalEpubStatus::InQueue)
        );
        let bp = studio.folder_state.batch_progress.as_ref().unwrap();
        assert!(bp.is_cancelled);
        assert!((bp.overall_progress() - 1.0).abs() < 1e-5);

        // Starting a new batch while a prior cancelled token exists signals/replaces it cleanly
        let stale_token = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        studio.batch_cancel_token = Some(stale_token);
        studio.folder_state.batch_progress = None;
        for item in &mut studio.folder_state.items {
            item.is_selected = true;
        }
        let prep2 = studio
            .try_prepare_batch_run(false)
            .expect("New batch after cancelled token should start");
        assert_eq!(prep2.run_generation, 2);
        assert!(!prep2.cancel_token.load(Ordering::SeqCst));
    }

    #[test]
    fn test_drop_pool_bounded_to_four_workers_and_incoming_dir_walk() {
        let tmp = tempfile::tempdir().expect("tempdir");
        for i in 1..=6 {
            std::fs::write(tmp.path().join(format!("Drop{i}.epub")), b"dummy").unwrap();
        }
        std::fs::write(tmp.path().join("Notes.txt"), b"txt").unwrap();

        let discovered = collect_incoming_epub_paths(tmp.path());
        assert_eq!(discovered.len(), 6);

        let mut studio = KindleStudio::new_for_test();
        let jobs = discovered
            .into_iter()
            .map(|(p, sz)| (p, Some(sz)))
            .collect();
        let to_spawn = studio
            .enqueue_drop_jobs_state(jobs, ConversionSettings::default())
            .expect("Should enqueue drop jobs");
        assert_eq!(
            to_spawn, 4,
            "Drop worker pool must cap at 4 concurrent workers"
        );
        assert_eq!(studio.drop_active_workers, 4);
        assert_eq!(studio.tasks.len(), 6);
    }

    #[test]
    fn test_superseded_device_refresh_discarded() {
        let mut studio = KindleStudio::new_for_test();
        studio.device_refresh_generation = 2;

        let stale_applied = studio.apply_device_refresh_result(
            1,
            DeviceStatus {
                connected: true,
                model: "Old Kindle".into(),
                path: Some(PathBuf::from("/old")),
                root_path: Some(PathBuf::from("/old")),
                used_gb: 1.0,
                total_gb: 8.0,
            },
            Vec::new(),
            false,
        );
        assert!(!stale_applied);
        assert!(!studio.device.connected);

        let current_applied = studio.apply_device_refresh_result(
            2,
            DeviceStatus {
                connected: true,
                model: "Kindle Paperwhite".into(),
                path: Some(PathBuf::from("/new/documents")),
                root_path: Some(PathBuf::from("/new")),
                used_gb: 2.0,
                total_gb: 16.0,
            },
            Vec::new(),
            false,
        );
        assert!(current_applied);
        assert!(studio.device.connected);
        assert_eq!(studio.device.model, "Kindle Paperwhite");
    }

    #[test]
    fn test_sort_column_transitions() {
        let mut field = EpubSortField::Title;
        let mut dir = SortDirection::Ascending;

        // Clicking active Title toggles Ascending -> Descending -> Ascending
        KindleStudio::apply_sort_column_transition(&mut field, &mut dir, EpubSortField::Title);
        assert_eq!(field, EpubSortField::Title);
        assert_eq!(dir, SortDirection::Descending);

        KindleStudio::apply_sort_column_transition(&mut field, &mut dir, EpubSortField::Title);
        assert_eq!(field, EpubSortField::Title);
        assert_eq!(dir, SortDirection::Ascending);

        // Clicking FileSize / DateModified starts Descending
        KindleStudio::apply_sort_column_transition(&mut field, &mut dir, EpubSortField::FileSize);
        assert_eq!(field, EpubSortField::FileSize);
        assert_eq!(dir, SortDirection::Descending);

        KindleStudio::apply_sort_column_transition(&mut field, &mut dir, EpubSortField::FileSize);
        assert_eq!(dir, SortDirection::Ascending);

        KindleStudio::apply_sort_column_transition(
            &mut field,
            &mut dir,
            EpubSortField::DateModified,
        );
        assert_eq!(field, EpubSortField::DateModified);
        assert_eq!(dir, SortDirection::Descending);

        // Clicking Author / Status starts Ascending
        KindleStudio::apply_sort_column_transition(&mut field, &mut dir, EpubSortField::Author);
        assert_eq!(field, EpubSortField::Author);
        assert_eq!(dir, SortDirection::Ascending);

        KindleStudio::apply_sort_column_transition(&mut field, &mut dir, EpubSortField::Status);
        assert_eq!(field, EpubSortField::Status);
        assert_eq!(dir, SortDirection::Ascending);
    }

    #[test]
    fn test_folder_view_cache_hit_and_invalidation() {
        let mut studio = KindleStudio::new_for_test();
        let path_a = PathBuf::from("/books/Zebra.epub");
        let path_b = PathBuf::from("/books/Alpha.epub");
        let mut item_a = dummy_item(1, path_a.clone(), true);
        item_a.title = "Zebra Tales".into();
        item_a.author = "Zane".into();
        item_a.file_size_bytes = 5_000;
        let mut item_b = dummy_item(2, path_b.clone(), false);
        item_b.title = "Alpha Café".into();
        item_b.author = "Alice".into();
        item_b.file_size_bytes = 1_000;

        studio.folder_state.items = vec![item_a, item_b];
        studio.mark_folder_items_dirty();
        assert_eq!(studio.folder_data_epoch, 1);

        // 1. Initial lookup computes [1, 0] (Alpha before Zebra), second lookup hits cache (same Arc pointer)
        let first = studio.cached_folder_indices();
        assert_eq!(&*first, &[1, 0]);
        let second = studio.cached_folder_indices();
        assert!(
            Arc::ptr_eq(&first, &second),
            "Unchanged folder view must return cached Arc pointer"
        );

        // 2. Sort column change invalidates cache
        KindleStudio::apply_sort_column_transition(
            &mut studio.folder_state.sort_field,
            &mut studio.folder_state.sort_direction,
            EpubSortField::FileSize,
        );
        let sorted_size = studio.cached_folder_indices();
        assert!(!Arc::ptr_eq(&first, &sorted_size));
        assert_eq!(&*sorted_size, &[0, 1]);

        // 3. Search query change (Unicode case-insensitive) invalidates cache
        studio.folder_state.search_query = "CAFÉ".into();
        let searched = studio.cached_folder_indices();
        assert!(!Arc::ptr_eq(&sorted_size, &searched));
        assert_eq!(&*searched, &[1]);

        // 4. Filter + item status mutation via apply_conversion_outcome bumps epoch and invalidates cache
        studio.folder_state.search_query.clear();
        studio.folder_state.filter_status = Some("converted".into());
        let before_convert = studio.cached_folder_indices();
        assert!(before_convert.is_empty());

        let epoch_before = studio.folder_data_epoch;
        studio.apply_conversion_outcome(
            99,
            &path_a,
            None,
            ConversionOutcome::Finished(ConversionResult {
                output_path: PathBuf::from("/books/Zebra.kfx"),
                total_bytes: 2048,
                synced_to_device: false,
                synced_device_path: None,
                sync_error: None,
                word_wise_glosses: None,
                word_wise_error: None,
                sdr_error: None,
            }),
        );
        assert!(studio.folder_data_epoch > epoch_before);
        let after_convert = studio.cached_folder_indices();
        assert!(!Arc::ptr_eq(&before_convert, &after_convert));
        assert_eq!(&*after_convert, &[0]);

        // 5. Batch prepare & cancel also bump epoch on item status transitions
        studio.folder_state.filter_status = Some("in_queue".into());
        studio.folder_state.items[1].is_selected = true;
        studio.mark_folder_items_dirty();
        let prep = studio.try_prepare_batch_run(false).expect("prepare batch");
        assert_eq!(&*studio.cached_folder_indices(), &[1]);
        assert!(prep.run_generation > 0);
        studio.batch_active_workers = 0;
        studio.cancel_batch_state();
        assert!(studio.cached_folder_indices().is_empty());
    }

    #[test]
    fn test_explorer_view_cache_hit_invalidation_and_counts() {
        let mut studio = KindleStudio::new_for_test();
        let make_file =
            |name: &str, title: &str, author: &str, format: &str, ww: bool, sz: u64| KindleFile {
                name: name.into(),
                title: title.into(),
                author: author.into(),
                format: format.into(),
                size_formatted: format!("{sz} B"),
                size_bytes: sz,
                modified: "Just now".into(),
                modified_timestamp: sz,
                full_path: PathBuf::from(format!("/kindle/documents/{name}")),
                has_word_wise: ww,
                has_sdr: true,
            };

        studio.device = DeviceStatus {
            connected: true,
            model: "Kindle".into(),
            path: Some(PathBuf::from("/kindle/documents")),
            root_path: Some(PathBuf::from("/kindle")),
            used_gb: 1.0,
            total_gb: 8.0,
        };
        studio.device_refresh_generation = 5;

        let files = vec![
            make_file("Zeta.kfx", "Zeta Story", "Unknown", "KFX", true, 500),
            make_file("Alpha.azw3", "Alpha Café", "Alice", "AZW3", false, 900),
            make_file("Beta.mobi", "Beta Book", "Bob", "MOBI", true, 200),
        ];
        assert!(studio.apply_device_refresh_result(5, studio.device.clone(), files, false));
        assert_eq!(studio.files_data_epoch, 1);

        // 1. Counts computed in single pass & cached Arc pointer reused on hit
        let (counts1, idx1) = studio.cached_explorer_view();
        assert_eq!(
            counts1,
            ExplorerFilterCounts {
                all: 3,
                kfx: 1,
                word_wise: 2,
                azw3: 1,
                mobi: 1,
            }
        );
        // Title Ascending: Alpha (1), Beta (2), Zeta (0)
        assert_eq!(&*idx1, &[1, 2, 0]);

        let (counts2, idx2) = studio.cached_explorer_view();
        assert_eq!(counts1, counts2);
        assert!(Arc::ptr_eq(&idx1, &idx2));

        // 2. Filter chip change invalidates cache while preserving accurate counts
        studio.filter_format = Some("WORDWISE".into());
        let (counts3, idx3) = studio.cached_explorer_view();
        assert_eq!(counts3, counts1);
        assert!(!Arc::ptr_eq(&idx1, &idx3));
        assert_eq!(&*idx3, &[2, 0]);

        // 3. Search query (Unicode case-insensitive) invalidates cache
        studio.filter_format = None;
        studio.search_query = "CAFÉ".into();
        let (_, idx4) = studio.cached_explorer_view();
        assert_eq!(&*idx4, &[1]);

        // 4. Sort column transition invalidates cache (Author ascending puts Unknown last)
        studio.search_query.clear();
        KindleStudio::apply_sort_column_transition(
            &mut studio.explorer_sort_field,
            &mut studio.explorer_sort_direction,
            EpubSortField::Author,
        );
        let (_, idx5) = studio.cached_explorer_view();
        assert_eq!(&*idx5, &[1, 2, 0]); // Alice, Bob, Unknown

        // 5. Synced conversion outcome updates self.files and bumps files_data_epoch
        let prev_epoch = studio.files_data_epoch;
        studio.apply_conversion_outcome(
            10,
            Path::new("/books/Gamma.epub"),
            None,
            ConversionOutcome::Finished(ConversionResult {
                output_path: PathBuf::from("/books/Gamma.kfx"),
                total_bytes: 3072,
                synced_to_device: true,
                synced_device_path: Some(PathBuf::from("/kindle/documents/Gamma.kfx")),
                sync_error: None,
                word_wise_glosses: Some(42),
                word_wise_error: None,
                sdr_error: None,
            }),
        );
        assert!(studio.files_data_epoch > prev_epoch);
        let (counts6, idx6) = studio.cached_explorer_view();
        assert!(!Arc::ptr_eq(&idx5, &idx6));
        assert_eq!(
            counts6,
            ExplorerFilterCounts {
                all: 4,
                kfx: 2,
                word_wise: 3,
                azw3: 1,
                mobi: 1,
            }
        );
    }

    #[test]
    fn test_is_batch_active_blocks_while_cancelled_workers_draining() {
        let mut studio = KindleStudio::new_for_test();
        studio.folder_state.items = vec![
            dummy_item(1, PathBuf::from("/books/One.epub"), true),
            dummy_item(2, PathBuf::from("/books/Two.epub"), true),
        ];

        let prep = studio
            .try_prepare_batch_run(false)
            .expect("Batch should prepare");
        assert_eq!(prep.num_workers, 2);
        assert_eq!(studio.batch_active_workers, 2);
        assert!(studio.is_batch_active());

        // Cancel while workers are still draining: is_batch_active() must stay true
        studio.cancel_batch_state();
        assert!(studio.is_batch_active());
        for item in &mut studio.folder_state.items {
            item.is_selected = true;
        }
        assert!(
            studio.try_prepare_batch_run(false).is_err(),
            "Must reject new batch while cancelled workers are still draining"
        );
        assert!(
            studio
                .try_prepare_single_convert(Path::new("/books/One.epub"), false)
                .is_err(),
            "Must reject single convert while cancelled batch workers are still draining"
        );

        // Once draining workers finish, batch becomes inactive and a new batch can start
        studio.batch_active_workers = 0;
        studio.batch_cancel_token = None;
        studio.batch_queue = None;
        assert!(!studio.is_batch_active());
        assert!(studio.try_prepare_batch_run(false).is_ok());
    }

    #[test]
    fn test_drop_worker_scheduling_incremental_global_cap_and_wakeup() {
        let mut studio = KindleStudio::new_for_test();
        studio.folder_state.items = vec![dummy_item(1, PathBuf::from("/books/Batch.epub"), true)];

        // 1. 1 active drop worker + 1 newly dropped book spawns 1 additional worker (not throttled to 0)
        let first_spawn = studio
            .enqueue_drop_jobs_state(
                vec![(PathBuf::from("/books/Drop1.epub"), Some("1 KB".into()))],
                ConversionSettings::default(),
            )
            .expect("Enqueue first drop job");
        assert_eq!(first_spawn, 1);
        assert_eq!(studio.drop_active_workers, 1);

        // Simulate active worker popping Drop1 so queue is empty while 1 worker is busy
        let popped = studio.drop_queue.lock().unwrap().pop_front();
        assert!(popped.is_some());

        let second_spawn = studio
            .enqueue_drop_jobs_state(
                vec![(PathBuf::from("/books/Drop2.epub"), Some("1 KB".into()))],
                ConversionSettings::default(),
            )
            .expect("Enqueue second drop job");
        assert_eq!(
            second_spawn, 1,
            "1 busy drop worker + 1 queued job must spawn 1 additional worker"
        );
        assert_eq!(studio.drop_active_workers, 2);

        // 2. Starting a batch while drop conversions are active is rejected
        assert!(
            studio.try_prepare_batch_run(false).is_err(),
            "Batch run must be rejected while drop workers/jobs are active"
        );

        // 3. Batch + drop obeys global MAX_TOTAL_CONVERSION_WORKERS (4)
        studio.drop_queue.lock().unwrap().clear();
        studio.drop_active_workers = 0;
        studio.batch_active_workers = 2;
        let under_batch_spawn = studio
            .enqueue_drop_jobs_state(
                vec![
                    (PathBuf::from("/books/D1.epub"), Some("1 KB".into())),
                    (PathBuf::from("/books/D2.epub"), Some("1 KB".into())),
                    (PathBuf::from("/books/D3.epub"), Some("1 KB".into())),
                ],
                ConversionSettings::default(),
            )
            .expect("Enqueue drop jobs while batch active");
        assert_eq!(under_batch_spawn, 2);
        assert_eq!(
            studio.batch_active_workers + studio.drop_active_workers,
            4,
            "Combined batch + drop active workers must never exceed 4"
        );

        // Additional drop job while saturated spawns 0 workers but still queues task
        let tasks_before = studio.tasks.len();
        let saturated_spawn = studio
            .enqueue_drop_jobs_state(
                vec![(PathBuf::from("/books/D4.epub"), Some("1 KB".into()))],
                ConversionSettings::default(),
            )
            .expect("Enqueue while saturated");
        assert_eq!(saturated_spawn, 0);
        assert_eq!(studio.tasks.len(), tasks_before + 1);

        // 4. Batch worker exit frees budget and wakes queued drop worker
        studio.batch_active_workers = 1;
        let woken_on_batch_exit = studio.claim_pending_drop_workers_state();
        assert_eq!(woken_on_batch_exit, 1);
        assert_eq!(studio.drop_active_workers, 3);
        assert_eq!(studio.batch_active_workers + studio.drop_active_workers, 4);

        // 5. Drop worker exit with remaining queued item wakes a replacement worker (zero stranding)
        studio.batch_active_workers = 0;
        studio.drop_active_workers = 0;
        let woken_on_drop_exit = studio.claim_pending_drop_workers_state();
        assert_eq!(woken_on_drop_exit, 4);
        assert_eq!(studio.drop_active_workers, 4);
    }

    #[test]
    fn test_batch_cancel_preserves_completed_inflight_outcome() {
        use super::resolve_batch_worker_outcome;

        let make_res = || ConversionResult {
            output_path: PathBuf::from("/books/Finished.kfx"),
            total_bytes: 8192,
            synced_to_device: true,
            synced_device_path: Some(PathBuf::from("/kindle/documents/Finished.kfx")),
            sync_error: None,
            word_wise_glosses: Some(12),
            word_wise_error: None,
            sdr_error: None,
        };

        // 1. Worker already finished with Ok(r) when cancel token became true -> preserve Finished(r)
        let outcome_ok_after_cancel = resolve_batch_worker_outcome(true, Ok(make_res()));
        assert!(matches!(
            outcome_ok_after_cancel,
            ConversionOutcome::Finished(_)
        ));

        // 2. Worker failed while cancelled -> Cancelled
        let outcome_err_after_cancel =
            resolve_batch_worker_outcome(true, Err("aborted mid-flight".into()));
        assert!(matches!(
            outcome_err_after_cancel,
            ConversionOutcome::Cancelled
        ));

        // 3. Worker failed without cancel -> Failed(msg)
        let outcome_err_normal = resolve_batch_worker_outcome(false, Err("corrupt epub".into()));
        assert!(matches!(
            outcome_err_normal,
            ConversionOutcome::Failed(ref msg) if msg == "corrupt epub"
        ));

        // 4. Applying Finished(r) after cancel still marks task Completed, item OnDevice, and adds to studio.files
        let mut studio = KindleStudio::new_for_test();
        let epub_path = PathBuf::from("/books/Finished.epub");
        studio.device = DeviceStatus {
            connected: true,
            model: "Kindle".into(),
            path: Some(PathBuf::from("/kindle/documents")),
            root_path: Some(PathBuf::from("/kindle")),
            used_gb: 1.0,
            total_gb: 8.0,
        };
        studio.folder_state.items = vec![dummy_item(1, epub_path.clone(), true)];
        let prep = studio
            .try_prepare_batch_run(true)
            .expect("Batch should prepare");
        let task_id = studio.tasks[0].id;
        // Simulate worker already in-flight (Converting / Processing) when user clicks Cancel
        studio.tasks[0].status = TaskStatus::Converting;
        studio.folder_state.items[0].status = LocalEpubStatus::Processing {
            stage: "Packaging & Sync".into(),
            progress: 80,
        };
        studio.cancel_batch_state();
        assert!(prep.cancel_token.load(Ordering::SeqCst));

        studio.apply_conversion_outcome(
            task_id,
            &epub_path,
            Some(prep.run_generation),
            outcome_ok_after_cancel,
        );
        assert_eq!(studio.tasks[0].status, TaskStatus::Completed);
        assert_eq!(
            studio.folder_state.items[0].status,
            LocalEpubStatus::OnDevice
        );
        assert_eq!(studio.files.len(), 1);
        assert_eq!(studio.files[0].name, "Finished.kfx");
    }

    #[test]
    fn test_rescan_folder_guard_blocks_during_active_batch_and_processing() {
        let mut studio = KindleStudio::new_for_test();
        let book_a = PathBuf::from("/books/A.epub");
        let book_b = PathBuf::from("/books/B.epub");
        studio.folder_state.items = vec![dummy_item(1, book_a.clone(), true)];

        // 1. Idle state allows rescan
        assert!(studio.can_start_folder_rescan().is_ok());

        // 2. Active batch blocks rescan
        let _prep = studio
            .try_prepare_batch_run(false)
            .expect("Batch should prepare");
        assert!(studio.can_start_folder_rescan().is_err());

        // 3. In-flight folder scan completing while conversion is active must NOT clobber items
        studio.folder_state.scan_generation = 7;
        studio.folder_state.is_scanning = true;
        studio.folder_state.scan_progress = Some(crate::state::ScanProgressState::discovering());
        let applied_during_batch =
            studio.apply_folder_scan_result(7, vec![dummy_item(99, book_b.clone(), false)]);
        assert!(!applied_during_batch);
        assert!(!studio.folder_state.is_scanning);
        assert!(studio.folder_state.scan_progress.is_none());
        assert_eq!(studio.folder_state.items.len(), 1);
        assert_eq!(studio.folder_state.items[0].path, book_a);

        // 4. Clear batch, leave single item Processing -> still blocks rescan & completion overwrite
        studio.batch_active_workers = 0;
        studio.batch_cancel_token = None;
        studio.batch_queue = None;
        studio.folder_state.batch_progress = None;
        studio.folder_state.items[0].status = LocalEpubStatus::Processing {
            stage: "Packaging KFX".into(),
            progress: 40,
        };
        assert!(studio.can_start_folder_rescan().is_err());
        assert!(!studio.apply_folder_scan_result(7, vec![dummy_item(99, book_b.clone(), false)]));
        assert_eq!(studio.folder_state.items[0].path, book_a);

        // 5. Stale generation does not overwrite items or clear newer scan state
        studio.folder_state.items[0].status = LocalEpubStatus::Unconverted;
        studio.folder_state.scan_generation = 8;
        studio.folder_state.is_scanning = true;
        assert!(!studio.apply_folder_scan_result(7, vec![dummy_item(99, book_b.clone(), false)]));
        assert!(studio.folder_state.is_scanning);
        assert_eq!(studio.folder_state.items[0].path, book_a);

        // 6. Idle + matching generation applies scan result cleanly
        assert!(studio.can_start_folder_rescan().is_ok());
        assert!(studio.apply_folder_scan_result(8, vec![dummy_item(99, book_b.clone(), false)]));
        assert!(!studio.folder_state.is_scanning);
        assert_eq!(studio.folder_state.items.len(), 1);
        assert_eq!(studio.folder_state.items[0].path, book_b);
    }

    #[test]
    fn test_tab_switch_coalescing_and_cooldown() {
        use super::TAB_REFRESH_COOLDOWN;
        use crate::state::StudioTab;
        use std::time::{Duration, Instant};

        let mut studio = KindleStudio::new_for_test();
        let t0 = Instant::now();

        // 1. Clicking the already-active tab (DualPane) is a no-op
        assert!(!studio.decide_tab_switch_refresh(StudioTab::DualPane, t0));
        assert_eq!(studio.tab, StudioTab::DualPane);

        // 2. Switching to a different tab when idle with no prior refresh triggers refresh
        assert!(studio.decide_tab_switch_refresh(StudioTab::Explorer, t0));
        assert_eq!(studio.tab, StudioTab::Explorer);

        // 3. In-flight refresh suppresses duplicate refresh while still switching tab
        studio.device_refresh_in_flight = true;
        assert!(!studio.decide_tab_switch_refresh(StudioTab::Convert, t0));
        assert_eq!(studio.tab, StudioTab::Convert);

        // Stale refresh generation must NOT clear in-flight flag for a newer refresh
        studio.device_refresh_generation = 3;
        assert!(!studio.apply_device_refresh_result(2, DeviceStatus::default(), Vec::new(), false));
        assert!(studio.device_refresh_in_flight);
        assert!(studio.last_device_refresh_at.is_none());

        // Matching generation clears in-flight and stamps last_device_refresh_at
        assert!(studio.apply_device_refresh_result(3, DeviceStatus::default(), Vec::new(), false));
        assert!(!studio.device_refresh_in_flight);
        assert!(studio.last_device_refresh_at.is_some());

        // 4. Within cooldown window -> switches tab without triggering refresh
        let stamped = studio.last_device_refresh_at.unwrap();
        let within_cooldown =
            stamped + TAB_REFRESH_COOLDOWN.saturating_sub(Duration::from_millis(1));
        assert!(!studio.decide_tab_switch_refresh(StudioTab::LocalFolder, within_cooldown));
        assert_eq!(studio.tab, StudioTab::LocalFolder);

        // 5. Past cooldown, eject_in_progress or active tasks still suppress refresh
        let after_cooldown = stamped + TAB_REFRESH_COOLDOWN;
        studio.eject_in_progress = true;
        assert!(!studio.decide_tab_switch_refresh(StudioTab::DualPane, after_cooldown));
        assert_eq!(studio.tab, StudioTab::DualPane);
        studio.eject_in_progress = false;

        studio.tasks.push(ConversionTask {
            id: 77,
            filename: "Busy.epub".into(),
            file_size_str: "2 KB".into(),
            status: TaskStatus::Converting,
            progress: 0.4,
            word_wise: false,
        });
        assert!(!studio.decide_tab_switch_refresh(StudioTab::Explorer, after_cooldown));
        assert_eq!(studio.tab, StudioTab::Explorer);
        studio.tasks.clear();

        // 6. Past cooldown and completely idle -> triggers refresh
        assert!(studio.decide_tab_switch_refresh(StudioTab::Convert, after_cooldown));
        assert_eq!(studio.tab, StudioTab::Convert);
    }

    #[test]
    fn test_drop_worker_auto_detect_triggers_device_file_refresh() {
        let mut studio = KindleStudio::new_for_test();
        studio.device_refresh_generation = 4;

        let connected_kindle = DeviceStatus {
            connected: true,
            model: "Kindle Paperwhite".into(),
            path: Some(PathBuf::from("/kindle/documents")),
            root_path: Some(PathBuf::from("/kindle")),
            used_gb: 2.0,
            total_gb: 16.0,
        };

        // 1. Fresh disconnected -> connected transition with matching generation returns true
        assert!(studio.should_apply_drop_detected_device(&connected_kindle, 4));

        // 2. Disconnected detected status returns false
        assert!(!studio.should_apply_drop_detected_device(&DeviceStatus::default(), 4));

        // 3. Stale generation (e.g. device refresh or eject ran concurrently) returns false
        assert!(!studio.should_apply_drop_detected_device(&connected_kindle, 3));

        // 4. Eject in progress returns false
        studio.eject_in_progress = true;
        assert!(!studio.should_apply_drop_detected_device(&connected_kindle, 4));
        studio.eject_in_progress = false;

        // 5. Already connected studio does not redundantly trigger transition
        studio.device = connected_kindle.clone();
        assert!(!studio.should_apply_drop_detected_device(&connected_kindle, 4));
    }
}
