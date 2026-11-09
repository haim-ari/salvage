#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod auth;
mod copies;
mod model;
mod restore;
mod store;

use futures::StreamExt;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{ipc::Response, AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;

use api::{Account, ApiError, Icloud, ROOT_ID};
use model::{Builder, Snapshot};
use restore::{Destination, Job};
use store::Store;

pub struct AppState {
    pub icloud: Icloud,
    pub store: Store,
    pub snapshot: RwLock<Option<Arc<Snapshot>>>,
    pub restored: Mutex<HashSet<String>>,
    pub job: Mutex<Option<Arc<AtomicBool>>>,
    /// The saved scan, read from disk on a thread while the window opens.
    warm: Mutex<Option<std::thread::JoinHandle<Warm>>>,
    /// The window JSON of the saved scan, made on that thread. Used once.
    first_view: Mutex<Option<String>>,
    /// The overview JSON of the saved scan, made on that thread. Used once.
    first_overview: Mutex<Option<String>>,
    /// Folders a restore to this Mac wrote to. "Show in Finder" opens only these.
    outputs: Mutex<HashSet<PathBuf>>,
}

/// What the launch thread reads from disk.
struct Warm {
    snapshot: Option<Snapshot>,
    restored: HashSet<String>,
    view: Option<String>,
    overview: Option<String>,
}

impl Warm {
    fn load(store: &Store) -> Self {
        let restored = store.restored();
        let snapshot = store.last_snapshot();
        let overview = snapshot.as_ref().map(overview_json);
        let view = snapshot.as_ref().map(|s| view_json(s, &restored));
        Self {
            snapshot,
            restored,
            view,
            overview,
        }
    }
}

impl AppState {
    /// Wait for the launch thread (once) and take what it read.
    /// The lock stays held while it waits, so no caller sees the state half filled.
    fn warm(&self) {
        let mut slot = self.warm.lock();
        let Some(handle) = slot.take() else { return };
        let Ok(w) = handle.join() else { return };
        self.restored.lock().extend(w.restored);
        let mut snap = self.snapshot.write();
        if snap.is_none() {
            *snap = w.snapshot.map(Arc::new);
            *self.first_view.lock() = w.view;
            *self.first_overview.lock() = w.overview;
        }
    }

    fn set_snapshot(&self, s: Option<Snapshot>) {
        *self.first_view.lock() = None;
        *self.first_overview.lock() = None;
        *self.snapshot.write() = s.map(Arc::new);
    }

    /// Keep the saved scan only if it belongs to this Apple Account.
    fn keep_snapshot_of(&self, dsid: &str) {
        let other = self
            .snapshot
            .read()
            .as_ref()
            .map(|s| s.dsid != dsid)
            .unwrap_or(false);
        if other {
            self.set_snapshot(None);
        }
        if self.snapshot.read().is_none() {
            if let Some(s) = self.store.snapshot(dsid) {
                self.set_snapshot(Some(s));
            }
        }
    }
}

type CmdResult<T> = Result<T, ApiError>;
type Page = Result<(Vec<Value>, Option<String>), ApiError>;

/// When the process started, for the startup timing in the log.
static START: OnceLock<Instant> = OnceLock::new();

fn since_start_ms() -> f64 {
    START
        .get()
        .map(|t| t.elapsed().as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// Log one startup step with the time since the process started.
fn log_step(stage: &str) {
    tracing::info!(target: "startup", stage, ms = since_start_ms(), "startup step");
}

/// The window reports its startup steps here.
#[tauri::command]
fn mark(stage: String) {
    log_step(&stage);
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    account: Option<Account>,
    version: String,
}

#[tauri::command]
fn app_state(app: AppHandle, state: State<AppState>) -> AppInfo {
    AppInfo {
        account: state.icloud.account(),
        version: app.package_info().version.to_string(),
    }
}

#[tauri::command]
fn sign_in(app: AppHandle) -> Result<(), String> {
    auth::open(&app).map_err(|e| e.to_string())
}

#[tauri::command]
async fn resume_session(app: AppHandle) -> Option<Account> {
    if !auth::resume(&app).await {
        return None;
    }
    let state = app.state::<AppState>();
    let account = state.icloud.account()?;
    state.warm();
    state.keep_snapshot_of(&account.dsid);
    Some(account)
}

#[tauri::command]
fn sign_out(app: AppHandle, state: State<AppState>) {
    state.warm();
    state.icloud.sign_out();
    state.store.clear();
    state.set_snapshot(None);
    state.restored.lock().clear();
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.clear_all_browsing_data();
    }
    let _ = app.emit("auth://changed", Option::<Account>::None);
}

/// What the window needs of one file. Hidden copies and fields only the restore uses stay here.
#[derive(Serialize)]
struct UiItem<'a> {
    i: &'a str,
    n: &'a str,
    e: &'a str,
    s: u64,
    d: i64,
    m: i64,
    c: i64,
    f: u32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    x: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UiSnapshot<'a> {
    scanned_at: i64,
    dsid: &'a str,
    items: Vec<UiItem<'a>>,
    folders: &'a [model::Folder],
    partial_paths: bool,
    copies_checked: bool,
}

#[derive(Serialize)]
struct View<'a> {
    snapshot: UiSnapshot<'a>,
    restored: &'a HashSet<String>,
}

fn view(state: &AppState) -> Response {
    let Some(snap) = state.snapshot.read().clone() else {
        return Response::new("null".to_string());
    };
    let restored = state.restored.lock();
    Response::new(view_json(&snap, &restored))
}

fn view_json(snap: &Snapshot, restored: &HashSet<String>) -> String {
    let items = snap
        .items
        .iter()
        .filter(|it| !it.u)
        .map(|it| UiItem {
            i: &it.i,
            n: &it.n,
            e: &it.e,
            s: it.s,
            d: it.d,
            m: it.m,
            c: it.c,
            f: it.f,
            x: it.x,
        })
        .collect();
    let snapshot = UiSnapshot {
        scanned_at: snap.scanned_at,
        dsid: &snap.dsid,
        items,
        folders: &snap.folders,
        partial_paths: snap.partial_paths,
        copies_checked: snap.copies_checked,
    };
    serde_json::to_string(&View { snapshot, restored }).unwrap_or_else(|_| "null".into())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedOverview<'a> {
    dsid: &'a str,
    scanned_at: i64,
    files: u64,
    bytes: u64,
    folders: Vec<model::FolderStat>,
}

/// Small JSON for the first screen: totals per folder, no file list.
fn overview_json(snap: &Snapshot) -> String {
    let folders = model::overview(snap);
    let files = snap.items.iter().filter(|it| !it.u).count() as u64;
    let bytes = folders.first().map(|f| f.bytes).unwrap_or(0);
    let o = SavedOverview {
        dsid: &snap.dsid,
        scanned_at: snap.scanned_at,
        files,
        bytes,
        folders,
    };
    serde_json::to_string(&o).unwrap_or_else(|_| "null".into())
}

/// The overview of the saved scan. The window paints this first, then loads the full list.
#[tauri::command]
fn get_overview(state: State<AppState>) -> Response {
    state.warm();
    if let Some(acc) = state.icloud.account() {
        state.keep_snapshot_of(&acc.dsid);
    }
    if let Some(o) = state.first_overview.lock().take() {
        return Response::new(o);
    }
    match state.snapshot.read().clone() {
        Some(snap) => Response::new(overview_json(&snap)),
        None => Response::new("null".to_string()),
    }
}

#[tauri::command]
fn get_snapshot(state: State<AppState>) -> Response {
    state.warm();
    // Before the sign-in check ends, the window shows the last saved scan.
    // After it, only a scan of the signed-in Apple Account is shown.
    if let Some(acc) = state.icloud.account() {
        state.keep_snapshot_of(&acc.dsid);
    }
    if let Some(v) = state.first_view.lock().take() {
        return Response::new(v);
    }
    view(&state)
}

/// Turn Apple's folder hierarchy reply into path parts below iCloud Drive.
fn hierarchy_parts(entry: &Value) -> Option<Vec<String>> {
    let is_root = |v: &Value| {
        v["drivewsid"]
            .as_str()
            .map(|d| d.ends_with("::root"))
            .unwrap_or(false)
    };
    let mut chain: Vec<&Value> = entry["hierarchy"]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    if chain.last().map(|l| is_root(l)).unwrap_or(false)
        && !chain.first().map(|f| is_root(f)).unwrap_or(false)
    {
        chain.reverse();
    }
    chain.push(entry);
    let mut parts = Vec::new();
    for node in chain {
        if is_root(node) {
            continue;
        }
        let name = node["name"].as_str()?;
        let full = match node["extension"].as_str() {
            Some(e) if !e.is_empty() && node["type"].as_str() != Some("FOLDER") => {
                format!("{name}.{e}")
            }
            _ => name.to_string(),
        };
        if parts.last() != Some(&full) {
            parts.push(full);
        }
    }
    Some(parts)
}

/// Ask Apple where the live folders that unplaced files need are. Ids that fail are tried
/// again after the next page.
async fn resolve_paths(
    app: &AppHandle,
    icloud: &Icloud,
    b: &Builder,
    resolved: &mut HashMap<String, Vec<String>>,
    tried: &mut HashSet<String>,
) -> Result<bool, ApiError> {
    let ids = b.wanted(tried);
    if ids.is_empty() {
        return Ok(false);
    }
    tried.extend(ids.iter().cloned());
    let total = ids.len();
    let mut done = 0usize;
    let mut jobs = Vec::new();
    for chunk in ids.chunks(50) {
        let chunk = chunk.to_vec();
        jobs.push(Box::pin(async move {
            let res = icloud.folders(&chunk, true).await;
            (chunk, res)
        }));
    }
    let mut results = futures::stream::iter(jobs).buffer_unordered(8);
    while let Some((chunk, res)) = results.next().await {
        done += chunk.len();
        if total > 200 {
            let _ = app.emit("scan://progress", serde_json::json!({ "count": b.items.len(), "phase": "paths", "done": done, "total": total }));
        }
        match res {
            Ok(entries) => {
                for e in entries {
                    let Some(id) = e["drivewsid"].as_str() else {
                        continue;
                    };
                    let parts = if id.ends_with("::root") {
                        Some(Vec::new())
                    } else {
                        hierarchy_parts(&e)
                    };
                    if let Some(parts) = parts {
                        resolved.insert(id.to_string(), parts);
                    }
                }
            }
            Err(ApiError::AuthExpired) => return Err(ApiError::AuthExpired),
            Err(e) => {
                tracing::warn!(target: "scan", error = %e, "folder lookup failed");
                for id in &chunk {
                    tried.remove(id);
                }
            }
        }
    }
    Ok(true)
}

#[derive(Serialize, Clone)]
struct Overview<'a> {
    files: usize,
    bytes: u64,
    pages: usize,
    complete: bool,
    unknown: usize,
    folders: &'a [model::FolderStat],
}

fn emit_overview(app: &AppHandle, b: &Builder, pages: usize, complete: bool) {
    let folders = b.stats();
    let bytes = folders.first().map(|f| f.bytes).unwrap_or(0);
    let _ = app.emit(
        "scan://folders",
        Overview {
            files: b.items.len(),
            bytes,
            pages,
            complete,
            unknown: b.unplaced.len(),
            folders: &folders,
        },
    );
}

#[tauri::command]
async fn scan(app: AppHandle, fresh: Option<bool>) -> CmdResult<Response> {
    let state = app.state::<AppState>();
    let account = state.icloud.account().ok_or(ApiError::NotSignedIn)?;
    state.warm();
    let icloud: &Icloud = &state.icloud;
    // A finished scan is kept. Only an explicit "Scan again" (fresh) reads Apple's list again.
    if !fresh.unwrap_or(false)
        && state
            .store
            .scan_state()
            .map(|s| s.complete && s.dsid == account.dsid)
            .unwrap_or(false)
    {
        if let Some(snap) = state.store.snapshot(&account.dsid) {
            state.set_snapshot(Some(snap));
            return Ok(view(&state));
        }
    }
    let mut b = Builder::new();
    if fresh.unwrap_or(false) {
        state.store.clear_paths();
    }
    let (mut resolved, mut tried) = state.store.paths(&account.dsid).unwrap_or_default();
    let dsid = account.dsid.clone();
    let store = &state.store;
    let save =
        |r: &HashMap<String, Vec<String>>, t: &HashSet<String>| store.save_paths(&dsid, r, t);

    // Continue a scan that stopped (app closed, crash) instead of starting again.
    let saved = state
        .store
        .scan_state()
        .filter(|s| {
            !fresh.unwrap_or(false)
                && s.dsid == account.dsid
                && !s.complete
                && s.pages > 0
                && s.marker.is_some()
        })
        .and_then(|s| state.store.scan_pages(s.pages).map(|p| (s, p)));
    let mut st = match saved {
        Some((s, pages)) => {
            for page in &pages {
                for d in page {
                    b.push(d);
                }
            }
            tracing::info!(target: "scan", pages = s.pages, items = b.items.len(), "resuming saved scan");
            let _ = app.emit(
                "scan://progress",
                serde_json::json!({ "count": b.items.len(), "phase": "list" }),
            );
            // Show what is known at once, then fill in folders not looked up yet.
            b.relayout(&resolved);
            emit_overview(&app, &b, s.pages, false);
            if resolve_paths(&app, icloud, &b, &mut resolved, &mut tried).await? {
                save(&resolved, &tried);
                b.relayout(&resolved);
                emit_overview(&app, &b, s.pages, false);
            }
            s
        }
        None => {
            state.store.scan_reset(&account.dsid);
            store::ScanState {
                dsid: account.dsid.clone(),
                ..Default::default()
            }
        }
    };
    let mut unverified = st.pages > 0;

    // The next page is asked for while the folders of this page are looked up.
    let mut fetched: Option<Page> = None;
    while !st.complete {
        let page = match fetched.take() {
            Some(p) => p,
            None => icloud.tombstone_page(st.marker.as_deref()).await,
        };
        let (docs, next) = match page {
            Ok(x) => x,
            Err(ApiError::AuthExpired) => return Err(ApiError::AuthExpired),
            Err(e) if unverified => {
                // Apple did not accept the saved marker: start the list again.
                tracing::warn!(target: "scan", error = %e, "saved marker refused, starting over");
                unverified = false;
                state.store.scan_reset(&account.dsid);
                st = store::ScanState {
                    dsid: account.dsid.clone(),
                    ..Default::default()
                };
                b = Builder::new();
                continue;
            }
            Err(e) => return Err(e),
        };
        unverified = false;
        for d in &docs {
            b.push(d);
        }
        st.pages += 1;
        st.complete = next.is_none();
        st.marker = next;
        state.store.scan_page(&st, &docs);
        let _ = app.emit(
            "scan://progress",
            serde_json::json!({ "count": b.items.len(), "phase": "list" }),
        );
        let looked_up = if st.complete {
            resolve_paths(&app, icloud, &b, &mut resolved, &mut tried).await
        } else {
            let (page, looked_up) = tokio::join!(
                icloud.tombstone_page(st.marker.as_deref()),
                resolve_paths(&app, icloud, &b, &mut resolved, &mut tried)
            );
            fetched = Some(page);
            looked_up
        };
        if looked_up? {
            save(&resolved, &tried);
        }
        b.relayout(&resolved);
        emit_overview(&app, &b, st.pages, st.complete);
    }

    // One more try for folders whose lookup failed earlier.
    if resolve_paths(&app, icloud, &b, &mut resolved, &mut tried).await? {
        save(&resolved, &tried);
    }
    b.relayout(&resolved);
    emit_overview(&app, &b, st.pages, true);
    tracing::info!(target: "scan", items = b.items.len(), dead_folders = b.dead_folders.len(), lookups = resolved.len(), unknown = b.unplaced.len(), "scan complete");

    let mut snap = b.finish(account.dsid.clone(), now_ms());
    copies::mark(&mut snap);
    tracing::info!(target: "scan", items = snap.items.len(), folders = snap.folders.len(), partial = snap.partial_paths, "snapshot built");
    state.store.save_snapshot(&snap);
    state.set_snapshot(Some(snap));
    Ok(view(&state))
}

/// Hide likely copies in the saved scan (no network).
#[tauri::command]
fn find_copies(state: State<AppState>) -> CmdResult<Response> {
    state.warm();
    let snap = state
        .snapshot
        .read()
        .clone()
        .ok_or_else(|| ApiError::Shape("scan first".into()))?;
    let mut snap = (*snap).clone();
    copies::mark(&mut snap);
    state.store.save_snapshot(&snap);
    state.set_snapshot(Some(snap));
    Ok(view(&state))
}

#[derive(Serialize)]
struct IcloudFolder {
    id: String,
    name: String,
    count: u64,
}

#[tauri::command]
async fn list_icloud_folder(
    state: State<'_, AppState>,
    id: Option<String>,
) -> CmdResult<Vec<IcloudFolder>> {
    let id = id.unwrap_or_else(|| ROOT_ID.to_string());
    let entries = state.icloud.folders(&[id], false).await?;
    let mut out: Vec<IcloudFolder> = entries
        .first()
        .and_then(|e| e["items"].as_array())
        .map(|a| {
            a.iter()
                .filter(|x| x["type"].as_str() == Some("FOLDER"))
                .filter_map(|x| {
                    Some(IcloudFolder {
                        id: x["drivewsid"].as_str()?.to_string(),
                        name: x["name"].as_str()?.to_string(),
                        count: x["directChildrenCount"]
                            .as_u64()
                            .or_else(|| x["numberOfItems"].as_u64())
                            .unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|f| f.name.to_lowercase());
    Ok(out)
}

#[tauri::command]
async fn create_icloud_folder(
    state: State<'_, AppState>,
    parent: String,
    name: String,
) -> CmdResult<String> {
    state.icloud.create_folder(&parent, &name).await
}

#[tauri::command]
async fn pick_local_folder(app: AppHandle) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Choose where to put the restored files")
            .blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
    .ok()
    .flatten()
}

#[tauri::command]
fn start_restore(
    app: AppHandle,
    state: State<AppState>,
    ids: Vec<String>,
    destination: Destination,
) -> CmdResult<String> {
    state.warm();
    let snap = state
        .snapshot
        .read()
        .clone()
        .ok_or_else(|| ApiError::Shape("scan first".into()))?;
    state.icloud.session()?;
    let mut job_slot = state.job.lock();
    if job_slot
        .as_ref()
        .map(|c| !c.load(std::sync::atomic::Ordering::Relaxed))
        .unwrap_or(false)
    {
        return Err(ApiError::Shape("a restore is already running".into()));
    }
    let wanted: HashSet<String> = ids.into_iter().collect();
    // Hidden copies always come back with the file that is shown for them.
    let items: Vec<_> = snap
        .items
        .iter()
        .filter(|i| wanted.contains(&i.i) || (!i.o.is_empty() && wanted.contains(&i.o)))
        .cloned()
        .collect();
    if items.is_empty() {
        return Err(ApiError::Shape("nothing selected".into()));
    }
    if let Destination::Local { path } = &destination {
        state.outputs.lock().insert(PathBuf::from(path));
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *job_slot = Some(cancel.clone());
    let id = uuid::Uuid::new_v4().to_string();
    let job = Job {
        id: id.clone(),
        items,
        folders: snap.folders.clone(),
        dest: destination,
        cancel: cancel.clone(),
    };
    tracing::info!(target: "restore", job = %id, count = job.items.len(), "restore started");
    tauri::async_runtime::spawn(async move {
        restore::run(app, job).await;
        // Mark the slot free.
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    Ok(id)
}

#[tauri::command]
fn cancel_restore(state: State<AppState>) {
    if let Some(c) = state.job.lock().as_ref() {
        c.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Open in Finder a folder that a restore wrote to. Any other path is refused, so the
/// window cannot make the app open files or apps.
#[tauri::command]
fn open_folder(state: State<AppState>, path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.is_absolute() || !p.is_dir() || !state.outputs.lock().contains(p) {
        tracing::warn!(target: "security", "open_folder refused a path that no restore wrote to");
        return Err("Salvage opens only folders it restored files to.".into());
    }
    std::process::Command::new("/usr/bin/open")
        .arg(p)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn init_logs() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_env("SALVAGE_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    if std::env::var("SALVAGE_LOG_PRETTY").is_ok() {
        fmt().with_env_filter(filter).init();
    } else {
        fmt()
            .json()
            .with_current_span(false)
            .with_env_filter(filter)
            .init();
    }
}

fn main() {
    START.get_or_init(Instant::now);
    init_logs();
    log_step("main");
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            log_step("setup-start");
            // Read the saved scan on a thread while the window opens.
            let store = Store::new(app.path().app_data_dir()?);
            let disk = store.clone();
            let warm = std::thread::spawn(move || {
                let w = Warm::load(&disk);
                log_step("warm");
                w
            });
            app.manage(AppState {
                icloud: Icloud::new(),
                store,
                snapshot: RwLock::new(None),
                restored: Mutex::new(HashSet::new()),
                job: Mutex::new(None),
                warm: Mutex::new(Some(warm)),
                first_view: Mutex::new(None),
                first_overview: Mutex::new(None),
                outputs: Mutex::new(HashSet::new()),
            });
            let cfg = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .cloned()
                .ok_or("no main window")?;
            tauri::WebviewWindowBuilder::from_config(app.handle(), &cfg)?.build()?;
            log_step("window");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            mark,
            app_state,
            sign_in,
            sign_out,
            resume_session,
            get_overview,
            get_snapshot,
            scan,
            find_copies,
            list_icloud_folder,
            create_icloud_folder,
            pick_local_folder,
            start_restore,
            cancel_restore,
            open_folder
        ])
        .build(tauri::generate_context!())
        .map(|app| {
            log_step("built");
            app.run(|_, _| {})
        })
        .expect("error while running Salvage");
}
