use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::KindleStudio;
use crate::state::{
    BatchProgressState, ConversionTask, DeviceStatus, EpubSortField, LocalEpubItem,
    LocalEpubStatus, ScanProgressState, SortDirection, TaskStatus, convert_epub_file,
    delete_device_file, detect_kindle_device, eject_kindle_device, load_device_files,
    scan_local_epub_directory_with_progress,
};
use gpui::*;
use kfx_converter::KindleBookFile;

impl KindleStudio {
    pub fn refresh_device(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let (device, files) = cx
                .background_executor()
                .spawn(async move {
                    std::thread::sleep(Duration::from_millis(150));
                    let dev = detect_kindle_device();
                    let f = load_device_files(&dev);
                    (dev, f)
                })
                .await;

            this.update(cx, |this, cx| {
                let mut f_list = files;
                for file in &mut f_list {
                    if file.author == "Unknown" {
                        let file_stem = file
                            .full_path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy();
                        if let Some(item) = this.folder_state.items.iter().find(|i| {
                            i.path.file_stem().map(|s| s.to_string_lossy())
                                == Some(file_stem.clone())
                                || i.title.eq_ignore_ascii_case(&file.title)
                        }) {
                            file.title = item.title.clone();
                            file.author = item.author.clone();
                        }
                    }
                }
                this.device = device;
                this.files = f_list;
                this.status_message = None;
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn eject_device(&mut self, cx: &mut Context<Self>) {
        let device = self.device.clone();
        self.status_message = Some(("Ejecting Kindle device...".into(), false));
        cx.notify();

        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move { eject_kindle_device(&device) })
                .await;

            this.update(cx, |this, cx| {
                match res {
                    Ok(_) => {
                        this.status_message = Some(("Kindle device safely ejected.".into(), false));
                        this.device = DeviceStatus::default();
                        this.files.clear();
                    }
                    Err(err) => {
                        this.status_message = Some((format!("Eject failed: {}", err), true));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn delete_file(&mut self, file_name: &str, cx: &mut Context<Self>) {
        if let Some(pos) = self.files.iter().position(|f| f.name == file_name) {
            let file = self.files[pos].clone();
            let file_name_owned = file_name.to_string();
            let Some(allowed_root) = self
                .device
                .path
                .clone()
                .or_else(|| self.device.root_path.clone())
            else {
                self.status_message = Some((
                    "Cannot delete file: Kindle storage root is not available.".into(),
                    true,
                ));
                cx.notify();
                return;
            };
            self.status_message = Some((format!("Deleting '{}'...", file_name), false));
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
                    match del_res {
                        Ok(_) => {
                            if let Some(idx) =
                                this.files.iter().position(|f| f.name == file_name_owned)
                            {
                                this.files.remove(idx);
                            }
                            this.status_message = Some((
                                format!("'{}' book and cache deleted.", file_name_owned),
                                false,
                            ));
                            this.device = new_device;
                        }
                        Err(err) => {
                            this.status_message = Some((err, true));
                        }
                    }
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
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

    pub fn rescan_folder(&mut self, cx: &mut Context<Self>) {
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
                this.folder_state.items = items;
                this.folder_state.is_scanning = false;
                this.folder_state.scan_progress = None;
                this.scan_cancel_token = None;
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
        cx.notify();
    }

    pub fn select_unconverted_only(&mut self, cx: &mut Context<Self>) {
        for item in &mut self.folder_state.items {
            item.is_selected = item.status == LocalEpubStatus::Unconverted;
        }
        cx.notify();
    }

    pub fn toggle_item_selection(&mut self, item_id: u64, cx: &mut Context<Self>) {
        if let Some(item) = self.folder_state.items.iter_mut().find(|i| i.id == item_id) {
            item.is_selected = !item.is_selected;
        }
        cx.notify();
    }

    pub fn cancel_batch_conversion(&mut self, cx: &mut Context<Self>) {
        if let Some(ref token) = self.batch_cancel_token {
            token.store(true, Ordering::SeqCst);
        }
        for item in self.folder_state.items.iter_mut() {
            if item.status == LocalEpubStatus::InQueue {
                item.status = LocalEpubStatus::Unconverted;
            }
        }
        if let Some(ref mut bp) = self.folder_state.batch_progress {
            bp.is_cancelled = true;
            bp.current_stage = "Cancelled remaining queue.".into();
        }
        self.status_message = Some(("Batch operation cancelled.".into(), false));
        cx.notify();
    }

    pub fn convert_selected_epubs(&mut self, sync_to_kindle: bool, cx: &mut Context<Self>) {
        let selected_paths: Vec<PathBuf> = self
            .folder_state
            .items
            .iter()
            .filter(|i| i.is_selected)
            .map(|i| i.path.clone())
            .collect();

        if selected_paths.is_empty() {
            self.status_message = Some(("Please select books to convert first.".into(), true));
            cx.notify();
            return;
        }

        let count = selected_paths.len();
        self.status_message = Some((
            format!(
                "Starting bounded conversion of {} books (2 concurrent workers)...",
                count
            ),
            false,
        ));

        if sync_to_kindle {
            self.settings.auto_deploy = true;
        }

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

        let mut queued_tasks = VecDeque::new();
        for path in &selected_paths {
            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let file_size_str = match std::fs::metadata(path) {
                Ok(meta) => {
                    let bytes = meta.len();
                    if bytes >= 1024 * 1024 {
                        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
                    } else {
                        format!("{} KB", bytes / 1024)
                    }
                }
                Err(_) => "Unknown".into(),
            };
            let task_id = (self.tasks.len() as u64) + (queued_tasks.len() as u64) + 1;
            queued_tasks.push_back((task_id, path.clone()));
            self.tasks.insert(
                0,
                ConversionTask {
                    id: task_id,
                    filename,
                    file_size_str,
                    status: TaskStatus::Queued,
                    progress: 0.0,
                    word_wise: self.settings.word_wise,
                    xray: false,
                },
            );
        }
        cx.notify();

        let settings = self.settings.clone();
        let device = self.device.clone();

        // Bounded worker pool: max 2 concurrent conversion workers to safeguard memory
        let queue = Arc::new(Mutex::new(queued_tasks));
        let num_workers = 2.min(selected_paths.len());

        for _ in 0..num_workers {
            let q = queue.clone();
            let s_clone = settings.clone();
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

                    let _ = this.update(cx, |this, cx| {
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
                        }
                        cx.notify();
                    });

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
                        if let Some(ref mut bp) = this.folder_state.batch_progress {
                            bp.active_tasks = bp.active_tasks.saturating_sub(1);
                        }
                        if cancel_check.load(Ordering::SeqCst) {
                            if let Some(t) = this.tasks.iter_mut().find(|t| t.id == task_id) {
                                t.status = TaskStatus::Failed("Batch conversion cancelled".into());
                            }
                            if let Some(item) = this
                                .folder_state
                                .items
                                .iter_mut()
                                .find(|i| i.path == path_buf)
                            {
                                item.status = LocalEpubStatus::Unconverted;
                            }
                            cx.notify();
                            return;
                        }
                        if let Some(ref mut bp) = this.folder_state.batch_progress {
                            if res.is_ok() {
                                bp.completed_tasks += 1;
                            } else {
                                bp.failed_tasks += 1;
                            }
                        }
                        if let Some(t) = this.tasks.iter_mut().find(|t| t.id == task_id) {
                            t.progress = 1.0;
                            match &res {
                                Ok(r) => {
                                    if r.synced_to_device {
                                        t.status = TaskStatus::Completed;
                                    } else {
                                        t.status = TaskStatus::ConvertedLocalOnly;
                                    }
                                }
                                Err(err) => {
                                    t.status = TaskStatus::Failed(err.clone());
                                }
                            }
                        }
                        if let Some(item) = this
                            .folder_state
                            .items
                            .iter_mut()
                            .find(|i| i.path == path_buf)
                        {
                            match &res {
                                Ok(r) => {
                                    if r.synced_to_device {
                                        item.status = LocalEpubStatus::OnDevice;
                                        let kfx_name = r
                                            .output_path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .to_string();
                                        if !this.files.iter().any(|f| f.name == kfx_name) {
                                            let dev_path = this
                                                .device
                                                .path
                                                .as_ref()
                                                .map(|p| p.join(&kfx_name))
                                                .unwrap_or_else(|| r.output_path.clone());
                                            let now_ts = SystemTime::now()
                                                .duration_since(UNIX_EPOCH)
                                                .map(|d| d.as_secs())
                                                .unwrap_or(0);
                                            this.files.push(KindleBookFile {
                                                name: kfx_name,
                                                title: item.title.clone(),
                                                author: item.author.clone(),
                                                format: "KFX".into(),
                                                size_formatted: "Synced".into(),
                                                size_bytes: 0,
                                                modified: "Just now".into(),
                                                modified_timestamp: now_ts,
                                                full_path: dev_path,
                                                has_word_wise: r
                                                    .word_wise_glosses
                                                    .is_some_and(|g| g > 0)
                                                    && r.word_wise_error.is_none(),
                                                has_sdr: true,
                                            });
                                        }
                                    } else {
                                        item.status = LocalEpubStatus::ConvertedLocal;
                                    }
                                }
                                Err(err) => {
                                    item.status = LocalEpubStatus::Error(err.clone());
                                }
                            }
                        }
                        cx.notify();
                    });
                }
            })
            .detach();
        }

        cx.notify();
    }

    pub fn convert_single_epub(
        &mut self,
        path: PathBuf,
        sync_to_kindle: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(item) = self.folder_state.items.iter_mut().find(|i| i.path == path) {
            item.status = LocalEpubStatus::Processing {
                stage: if sync_to_kindle {
                    "Packaging & Sync".into()
                } else {
                    "Packaging KFX".into()
                },
                progress: 30,
            };
        }
        if sync_to_kindle {
            self.settings.auto_deploy = true;
        }
        self.process_epub_file(&path, cx);
        cx.notify();
    }

    pub fn handle_incoming_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        if path.is_file() {
            if path
                .extension()
                .and_then(|s| s.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("epub"))
                .unwrap_or(false)
            {
                self.process_epub_file(path, cx);
            }
        } else if path.is_dir()
            && let Ok(entries) = std::fs::read_dir(path)
        {
            for entry in entries.flatten() {
                let child_path = entry.path();
                if child_path.is_file()
                    && child_path
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|ext| ext.eq_ignore_ascii_case("epub"))
                        .unwrap_or(false)
                {
                    self.process_epub_file(&child_path, cx);
                }
            }
        }
    }

    pub fn process_epub_file(&mut self, path: &Path, cx: &mut Context<Self>) {
        let filename = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let file_size_str = match std::fs::metadata(path) {
            Ok(meta) => {
                let bytes = meta.len();
                if bytes >= 1024 * 1024 {
                    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
                } else {
                    format!("{} KB", bytes / 1024)
                }
            }
            Err(_) => "Unknown".into(),
        };

        let task_id = (self.tasks.len() as u64) + 1;
        self.tasks.insert(
            0,
            ConversionTask {
                id: task_id,
                filename: filename.clone(),
                file_size_str,
                status: TaskStatus::Converting,
                progress: 0.5,
                word_wise: self.settings.word_wise,
                xray: false,
            },
        );
        cx.notify();

        self.device = detect_kindle_device();

        let path_buf = path.to_path_buf();
        let target_path = path_buf.clone();
        let settings = self.settings.clone();
        let device = self.device.clone();

        cx.spawn(async move |this, cx| {
            let task = cx.background_executor().spawn(async move {
                convert_epub_file(&path_buf, &settings, &device, None::<fn(f32, &str)>)
            });
            let res = task.await;

            this.update(cx, |this, cx| {
                if let Some(t) = this.tasks.iter_mut().find(|t| t.id == task_id) {
                    t.progress = 1.0;
                    match res {
                        Ok(res) => {
                            if let Some(ref ww_err) = res.word_wise_error {
                                this.status_message = Some((ww_err.clone(), true));
                            }
                            if res.synced_to_device {
                                t.status = TaskStatus::Completed;
                            } else if let Some(err) = res.sync_error {
                                t.status = TaskStatus::SyncFailed(err);
                            } else {
                                t.status = TaskStatus::ConvertedLocalOnly;
                            }
                            if res.synced_to_device {
                                let kfx_name = res
                                    .output_path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string();
                                if !this.files.iter().any(|f| f.name == kfx_name) {
                                    let dev_path = this
                                        .device
                                        .path
                                        .as_ref()
                                        .map(|p| p.join(&kfx_name))
                                        .unwrap_or_else(|| res.output_path.clone());
                                    let (item_title, item_author) = if let Some(item) = this
                                        .folder_state
                                        .items
                                        .iter()
                                        .find(|i| i.path == target_path)
                                    {
                                        (item.title.clone(), item.author.clone())
                                    } else {
                                        (kfx_name.clone(), "Unknown".into())
                                    };
                                    let now_ts = SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .map(|d| d.as_secs())
                                        .unwrap_or(0);
                                    this.files.push(KindleBookFile {
                                        name: kfx_name,
                                        title: item_title,
                                        author: item_author,
                                        format: "KFX".into(),
                                        size_formatted: "Synced".into(),
                                        size_bytes: 0,
                                        modified: "Just now".into(),
                                        modified_timestamp: now_ts,
                                        full_path: dev_path,
                                        has_word_wise: res.word_wise_glosses.is_some_and(|g| g > 0)
                                            && res.word_wise_error.is_none(),
                                        has_sdr: true,
                                    });
                                }
                            }

                            if let Some(item) = this
                                .folder_state
                                .items
                                .iter_mut()
                                .find(|i| i.path == target_path)
                            {
                                item.status = if res.synced_to_device {
                                    LocalEpubStatus::OnDevice
                                } else {
                                    LocalEpubStatus::ConvertedLocal
                                };
                                item.is_selected = false;
                            }
                        }
                        Err(err) => {
                            t.status = TaskStatus::Failed(err.clone());
                            if let Some(item) = this
                                .folder_state
                                .items
                                .iter_mut()
                                .find(|i| i.path == target_path)
                            {
                                item.status = LocalEpubStatus::Error(err);
                            }
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn handle_sort_column_click(
        &mut self,
        field: EpubSortField,
        cx: &mut Context<Self>,
    ) {
        if self.folder_state.sort_field == field {
            self.folder_state.sort_direction = self.folder_state.sort_direction.toggle();
        } else {
            self.folder_state.sort_field = field;
            self.folder_state.sort_direction = match field {
                EpubSortField::DateModified | EpubSortField::FileSize => SortDirection::Descending,
                _ => SortDirection::Ascending,
            };
        }
        cx.notify();
    }

    pub(super) fn handle_explorer_sort_column_click(
        &mut self,
        field: EpubSortField,
        cx: &mut Context<Self>,
    ) {
        if self.explorer_sort_field == field {
            self.explorer_sort_direction = self.explorer_sort_direction.toggle();
        } else {
            self.explorer_sort_field = field;
            self.explorer_sort_direction = match field {
                EpubSortField::DateModified | EpubSortField::FileSize => SortDirection::Descending,
                _ => SortDirection::Ascending,
            };
        }
        cx.notify();
    }
}
