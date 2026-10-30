pub mod dictionary;
pub mod display_time;
pub mod enum_cache;
pub mod enum_count;
pub mod enum_export;
pub mod enum_key;
pub mod enum_store;
pub mod enums;
pub mod export_csv;
pub mod export_xlsx;
pub mod files;
mod footer;
pub mod format;
pub mod meta;
pub mod sample;
pub mod settings;
mod task;
pub mod value;

use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

#[derive(Default)]
struct Files {
    next: AtomicU64,
    entries: Mutex<BTreeMap<u64, Arc<meta::FileMeta>>>,
}
struct EnumRuntime {
    cache: enum_cache::Cache,
    store: enum_store::Store,
}
impl Default for EnumRuntime {
    fn default() -> Self {
        let base = std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("XDG_CACHE_HOME"))
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        Self {
            cache: enum_cache::Cache::new(base.join("ParquetLens").join("cache")),
            store: enum_store::Store::default(),
        }
    }
}
#[derive(Serialize)]
struct OpenedFile {
    id: String,
    #[serde(flatten)]
    info: meta::FileMeta,
}

#[tauri::command]
async fn open_file(
    path: String,
    state: tauri::State<'_, Files>,
    prefs: tauri::State<'_, settings::Settings>,
) -> Result<OpenedFile, files::OpenError> {
    let info = tauri::async_runtime::spawn_blocking(move || files::inspect_file(&path))
        .await
        .map_err(|e| meta::failure("task_failed", e))??;
    let id = state.next.fetch_add(1, Ordering::Relaxed) + 1;
    let mut entries = state
        .entries
        .lock()
        .map_err(|e| meta::failure("task_failed", e))?;
    let info = Arc::new(info);
    if let Some((&existing, old)) = entries.iter().find(|(_, m)| {
        if cfg!(windows) {
            m.path.eq_ignore_ascii_case(&info.path)
        } else {
            m.path == info.path
        }
    }) {
        if old.modified != info.modified || old.size_bytes != info.size_bytes {
            return Err(meta::failure(
                "file_changed",
                "Close and reopen changed file",
            ));
        }
        let mut first = (**old).clone();
        first.columns.truncate(500);
        drop(entries);
        let _ = prefs.recent(Some(info.path.clone()));
        return Ok(OpenedFile {
            id: existing.to_string(),
            info: first,
        });
    }
    if entries.len() >= 10 {
        return Err(meta::failure(
            "tab_limit",
            "Close a tab before opening more than 10 files",
        ));
    }
    entries.insert(id, info.clone());
    drop(entries);
    let _ = prefs.recent(Some(info.path.clone()));
    let mut first = (*info).clone();
    first.columns.truncate(500);
    Ok(OpenedFile {
        id: id.to_string(),
        info: first,
    })
}

#[tauri::command]
fn get_columns(
    id: String,
    offset: usize,
    limit: usize,
    state: tauri::State<'_, Files>,
) -> Result<Vec<meta::ColumnInfo>, files::OpenError> {
    if !(1..=500).contains(&limit) {
        return Err(meta::failure(
            "invalid_request",
            "Page limit must be 1..500",
        ));
    }
    let id = id
        .parse::<u64>()
        .map_err(|e| meta::failure("invalid_request", e))?;
    let entries = state
        .entries
        .lock()
        .map_err(|e| meta::failure("task_failed", e))?;
    let info = entries
        .get(&id)
        .ok_or_else(|| meta::failure("invalid_request", "File session is closed"))?;
    Ok(info
        .columns
        .iter()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect())
}

#[tauri::command]
async fn close_file(
    id: String,
    state: tauri::State<'_, Files>,
    tasks: tauri::State<'_, Arc<task::Tasks>>,
    enums: tauri::State<'_, Arc<EnumRuntime>>,
) -> Result<(), files::OpenError> {
    let worker = tasks.inner().clone();
    let owner = id.clone();
    tauri::async_runtime::spawn_blocking(move || worker.cancel_and_wait(&owner))
        .await
        .map_err(|e| meta::failure("task_failed", e))??;
    enums.store.close(&id);
    let id = id
        .parse::<u64>()
        .map_err(|e| meta::failure("invalid_request", e))?;
    state
        .entries
        .lock()
        .map_err(|e| meta::failure("task_failed", e))?
        .remove(&id);
    Ok(())
}
#[derive(Default)]
struct Startup(Mutex<Vec<String>>);
#[tauri::command]
fn startup_paths(paths: tauri::State<'_, Startup>) -> Vec<String> {
    std::mem::take(&mut *paths.0.lock().unwrap())
}

fn snapshot(state: &Files, id: &str) -> Result<Arc<meta::FileMeta>, files::OpenError> {
    let id = id
        .parse::<u64>()
        .map_err(|e| meta::failure("invalid_request", e))?;
    state
        .entries
        .lock()
        .map_err(|e| meta::failure("task_failed", e))?
        .get(&id)
        .cloned()
        .ok_or_else(|| meta::failure("invalid_request", "File session is closed"))
}
#[tauri::command]
async fn sample_fields(
    id: String,
    state: tauri::State<'_, Files>,
) -> Result<Vec<sample::Field>, files::OpenError> {
    let info = snapshot(&state, &id)?;
    tauri::async_runtime::spawn_blocking(move || sample::fields(&info))
        .await
        .map_err(|e| meta::failure("task_failed", e))?
}
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Explicit IPC fields plus injected Tauri state.
async fn get_preview(
    id: String,
    options: sample::Request,
    offset: usize,
    limit: usize,
    request: String,
    state: tauri::State<'_, Files>,
    prefs: tauri::State<'_, settings::Settings>,
    tasks: tauri::State<'_, Arc<task::Tasks>>,
) -> Result<sample::Preview, files::OpenError> {
    let local = prefs.snapshot().preferences.time_mode == "local";
    let info = snapshot(&state, &id)?;
    let guard = tasks.begin(id, request)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _time = display_time::Scope::new(local);
        let result = sample::preview(&info, &options, offset, limit, &guard.job.cancel);
        drop(guard);
        result
    })
    .await
    .map_err(|e| meta::failure("task_failed", e))?
}
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Explicit IPC fields plus injected Tauri state.
async fn export_xlsx(
    id: String,
    options: sample::Request,
    out_path: String,
    request: String,
    state: tauri::State<'_, Files>,
    prefs: tauri::State<'_, settings::Settings>,
    tasks: tauri::State<'_, Arc<task::Tasks>>,
    app: tauri::AppHandle,
) -> Result<export_xlsx::Report, files::OpenError> {
    use tauri::Emitter;
    let local = prefs.snapshot().preferences.time_mode == "local";
    let info = snapshot(&state, &id)?;
    let guard = tasks.begin(id, request.clone())?;
    tauri::async_runtime::spawn_blocking(move||{
        let _time=display_time::Scope::new(local);
        let mut last=std::time::Instant::now();
        let result=export_xlsx::export(&info,&options,std::path::Path::new(&out_path),&guard.job.cancel,|done,phase| {
            if last.elapsed().as_millis()>=100 || phase=="packaging" {
                let _=app.emit("sample_progress",serde_json::json!({"request":request,"done":done,"total":options.n.min(info.num_rows.parse::<usize>().unwrap_or(0)),"phase":phase}));
                last=std::time::Instant::now();
            }
        });
        drop(guard);result
    }).await.map_err(|e|meta::failure("task_failed",e))?
}
#[tauri::command]
fn cancel_task(
    id: String,
    request: Option<String>,
    tasks: tauri::State<'_, Arc<task::Tasks>>,
) -> Result<(), files::OpenError> {
    tasks.cancel(&id, request.as_deref())
}

#[derive(Serialize)]
struct EnumOutcome {
    results: Vec<enums::EnumResult>,
    warnings: Vec<String>,
    cancelled: bool,
}
#[tauri::command]
async fn enum_catalog(
    id: String,
    state: tauri::State<'_, Files>,
) -> Result<Vec<enums::Column>, files::OpenError> {
    let meta = snapshot(&state, &id)?;
    tauri::async_runtime::spawn_blocking(move || enums::catalog(&meta))
        .await
        .map_err(|e| meta::failure("task_failed", e))?
}
#[tauri::command]
async fn analyze_enums(
    id: String,
    options: enums::Options,
    request: String,
    state: tauri::State<'_, Files>,
    tasks: tauri::State<'_, Arc<task::Tasks>>,
    runtime: tauri::State<'_, Arc<EnumRuntime>>,
    app: tauri::AppHandle,
) -> Result<EnumOutcome, files::OpenError> {
    use tauri::Emitter;
    let meta = snapshot(&state, &id)?;
    let guard = tasks.begin(id.clone(), request.clone())?;
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            enums::analyze(
                &meta,
                &options,
                &guard.job.cancel,
                &runtime.cache,
                |r| {
                    if let Ok(summary) = runtime.store.put(&id, &request, r) {
                        let _ = app.emit(
                            "enum_result",
                            serde_json::json!({"request":request,"result":summary}),
                        );
                    }
                },
                |p| {
                    let _ = app.emit(
                        "enum_progress",
                        serde_json::json!({"request":request,"progress":p}),
                    );
                },
            )
        }))
        .unwrap_or_else(|_| Err(meta::failure("task_failed", "Enumeration task panicked")));
        drop(guard);
        match result {
            Ok(out) => {
                let results = out
                    .results
                    .into_iter()
                    .map(|r| {
                        let mut r = r.summary();
                        r.id = format!("{request}:{}", r.column.root);
                        r
                    })
                    .collect();
                let out = EnumOutcome {
                    results,
                    warnings: out.warnings,
                    cancelled: out.cancelled,
                };
                let _ = app.emit(
                    "enum_done",
                    serde_json::json!({"request":request,"cancelled":out.cancelled}),
                );
                Ok(out)
            }
            Err(e) => {
                runtime.store.close(&id);
                let _ = app.emit(
                    "enum_done",
                    serde_json::json!({"request":request,"error":e.code}),
                );
                Err(e)
            }
        }
    })
    .await
    .map_err(|e| meta::failure("task_failed", e))?
}
#[derive(serde::Deserialize)]
struct EnumPageOptions {
    offset: usize,
    limit: usize,
    search: String,
    sort: String,
    descending: bool,
}
#[tauri::command]
async fn enum_values(
    id: String,
    result_id: String,
    options: EnumPageOptions,
    state: tauri::State<'_, Files>,
    prefs: tauri::State<'_, settings::Settings>,
    runtime: tauri::State<'_, Arc<EnumRuntime>>,
) -> Result<enum_store::Page, files::OpenError> {
    let local = prefs.snapshot().preferences.time_mode == "local";
    let meta = snapshot(&state, &id)?;
    let result = runtime.store.get(&id, &result_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        sample::unchanged(&meta)?;
        let mut displayed = std::borrow::Cow::Borrowed(result.as_ref());
        if local {
            let result = displayed.to_mut();
            let _time = display_time::Scope::new(true);
            let builder = sample::builder(&meta)?;
            for v in &mut result.values {
                if let Some(s) = &mut v.value {
                    *s = display_time::enum_text(
                        s,
                        builder.schema().field(result.column.root).data_type(),
                    );
                }
            }
        }
        let page = enum_store::page(
            &displayed,
            options.offset,
            options.limit,
            &options.search,
            &options.sort,
            options.descending,
        )?;
        Ok(page)
    })
    .await
    .map_err(|e| meta::failure("task_failed", e))?
}
#[tauri::command]
async fn clear_enum_cache(
    runtime: tauri::State<'_, Arc<EnumRuntime>>,
) -> Result<(), files::OpenError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        runtime.cache.clear()?;
        runtime.store.clear();
        Ok(())
    })
    .await
    .map_err(|e| meta::failure("task_failed", e))?
}
#[tauri::command]
async fn enum_cache_info(
    runtime: tauri::State<'_, Arc<EnumRuntime>>,
) -> Result<serde_json::Value, files::OpenError> {
    let runtime = runtime.inner().clone();
    tauri::async_runtime::spawn_blocking(move||Ok(serde_json::json!({"bytes":runtime.cache.size()?,"limit":enum_cache::LIMIT,"path":runtime.cache.dir}))).await.map_err(|e|meta::failure("task_failed",e))?
}
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Explicit IPC fields plus injected Tauri state.
async fn export_enum_xlsx(
    id: String,
    result_id: String,
    out_path: String,
    request: String,
    state: tauri::State<'_, Files>,
    prefs: tauri::State<'_, settings::Settings>,
    tasks: tauri::State<'_, Arc<task::Tasks>>,
    runtime: tauri::State<'_, Arc<EnumRuntime>>,
) -> Result<export_xlsx::Report, files::OpenError> {
    let local = prefs.snapshot().preferences.time_mode == "local";
    let meta = snapshot(&state, &id)?;
    let result = runtime.store.get(&id, &result_id)?;
    let guard = tasks.begin(id, request)?;
    tauri::async_runtime::spawn_blocking(move || {
        let _time = display_time::Scope::new(local);
        let r = enum_export::export(
            &meta,
            &result,
            std::path::Path::new(&out_path),
            &guard.job.cancel,
        );
        drop(guard);
        r
    })
    .await
    .map_err(|e| meta::failure("task_failed", e))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri::Manager;
    tauri::Builder::default()
        .manage(settings::Settings::default())
        .manage(Startup(Mutex::new(
            std::env::args_os()
                .skip(1)
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
        )))
        .plugin(tauri_plugin_single_instance::init(|app, args, cwd| {
            use tauri::Emitter;
            let pending = args
                .into_iter()
                .skip(1)
                .map(|p| {
                    let p = std::path::PathBuf::from(p);
                    if p.is_absolute() {
                        p
                    } else {
                        std::path::Path::new(&cwd).join(p)
                    }
                    .to_string_lossy()
                    .into_owned()
                })
                .collect::<Vec<_>>();
            app.state::<Startup>().0.lock().unwrap().extend(pending);
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            let _ = app.emit("open_requests", ());
        }))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let tasks = window.state::<Arc<task::Tasks>>().inner().clone();
                if let Some(job) = tasks.status() {
                    api.prevent_close();
                    let owner = job["owner"].as_str().unwrap_or_default().to_string();
                    let window = window.clone();
                    tauri::async_runtime::spawn_blocking(move || {
                        let _ = tasks.cancel_and_wait(&owner);
                        let _ = window.close();
                    });
                }
            }
        })
        .setup(|app| {
            app.state::<Arc<task::Tasks>>().attach(app.handle().clone());
            Ok(())
        })
        .manage(Files::default())
        .manage(Arc::new(task::Tasks::default()))
        .manage(Arc::new(EnumRuntime::default()))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            open_file,
            get_columns,
            close_file,
            startup_paths,
            sample_fields,
            get_preview,
            export_xlsx,
            cancel_task,
            enum_catalog,
            analyze_enums,
            enum_values,
            clear_enum_cache,
            enum_cache_info,
            export_enum_xlsx,
            get_settings,
            save_settings,
            clear_recent,
            display_columns,
            task_status,
            export_sample_csv,
            export_enum_csv,
            export_dictionary
        ])
        .run(tauri::generate_context!())
        .expect("failed to start ParquetLens");
}
#[tauri::command]
fn get_settings(prefs: tauri::State<'_, settings::Settings>) -> settings::Snapshot {
    prefs.snapshot()
}
#[tauri::command]
fn save_settings(
    preferences: settings::Preferences,
    prefs: tauri::State<'_, settings::Settings>,
) -> sample::Result<settings::Snapshot> {
    prefs.save(preferences)
}
#[tauri::command]
fn clear_recent(prefs: tauri::State<'_, settings::Settings>) -> sample::Result<settings::Snapshot> {
    prefs.recent(None)
}
#[tauri::command]
fn task_status(tasks: tauri::State<'_, Arc<task::Tasks>>) -> Option<serde_json::Value> {
    tasks.status()
}
#[tauri::command]
fn display_columns(
    id: String,
    offset: usize,
    limit: usize,
    state: tauri::State<'_, Files>,
    prefs: tauri::State<'_, settings::Settings>,
) -> sample::Result<Vec<meta::ColumnInfo>> {
    if !(1..=500).contains(&limit) {
        return Err(meta::failure("invalid_request", "Page limit 1..500"));
    }
    let info = snapshot(&state, &id)?;
    let mut cols = info
        .columns
        .iter()
        .skip(offset)
        .take(limit)
        .cloned()
        .collect::<Vec<_>>();
    display_time::columns(&mut cols, prefs.snapshot().preferences.time_mode == "local");
    Ok(cols)
}
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Explicit IPC fields plus injected Tauri state.
async fn export_sample_csv(
    id: String,
    options: sample::Request,
    out_path: String,
    protect: bool,
    request: String,
    state: tauri::State<'_, Files>,
    tasks: tauri::State<'_, Arc<task::Tasks>>,
    prefs: tauri::State<'_, settings::Settings>,
    app: tauri::AppHandle,
) -> sample::Result<export_xlsx::Report> {
    use tauri::Emitter;
    let meta = snapshot(&state, &id)?;
    let guard = tasks.begin(id, request.clone())?;
    let local = prefs.snapshot().preferences.time_mode == "local";
    tauri::async_runtime::spawn_blocking(move||{let _time=display_time::Scope::new(local);let mut last=std::time::Instant::now();export_csv::sample_export(&meta,&options,std::path::Path::new(&out_path),protect,&guard.job.cancel,|done|{if last.elapsed().as_millis()>=100{let _=app.emit("sample_progress",serde_json::json!({"request":request,"done":done,"total":options.n,"phase":"writing"}));last=std::time::Instant::now();}})}).await.map_err(|e|meta::failure("task_failed",e))?
}
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Explicit IPC fields plus injected Tauri state.
async fn export_enum_csv(
    id: String,
    result_id: String,
    out_path: String,
    protect: bool,
    request: String,
    state: tauri::State<'_, Files>,
    tasks: tauri::State<'_, Arc<task::Tasks>>,
    runtime: tauri::State<'_, Arc<EnumRuntime>>,
    prefs: tauri::State<'_, settings::Settings>,
) -> sample::Result<export_xlsx::Report> {
    let meta = snapshot(&state, &id)?;
    let result = runtime.store.get(&id, &result_id)?;
    let guard = tasks.begin(id, request)?;
    let local = prefs.snapshot().preferences.time_mode == "local";
    tauri::async_runtime::spawn_blocking(move || {
        let _time = display_time::Scope::new(local);
        export_csv::enum_export(
            &meta,
            &result,
            std::path::Path::new(&out_path),
            protect,
            &guard.job.cancel,
        )
    })
    .await
    .map_err(|e| meta::failure("task_failed", e))?
}
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Explicit IPC fields plus injected Tauri state.
async fn export_dictionary(
    id: String,
    options: dictionary::Request,
    out_path: String,
    request: String,
    state: tauri::State<'_, Files>,
    tasks: tauri::State<'_, Arc<task::Tasks>>,
    runtime: tauri::State<'_, Arc<EnumRuntime>>,
    prefs: tauri::State<'_, settings::Settings>,
    app: tauri::AppHandle,
) -> sample::Result<export_xlsx::Report> {
    use tauri::Emitter;
    let meta = snapshot(&state, &id)?;
    let existing = runtime.store.latest(&id);
    let guard = tasks.begin(id, request.clone())?;
    let runtime = runtime.inner().clone();
    let local = prefs.snapshot().preferences.time_mode == "local";
    tauri::async_runtime::spawn_blocking(move || {
        dictionary::export(
            &meta,
            &options,
            std::path::Path::new(&out_path),
            dictionary::Context {
                local,
                cache: &runtime.cache,
                existing,
            },
            &guard.job.cancel,
            |done, phase| {
                let _ = app.emit(
                    "dictionary_progress",
                    serde_json::json!({"request":request,"done":done,"phase":phase}),
                );
            },
        )
    })
    .await
    .map_err(|e| meta::failure("task_failed", e))?
}
