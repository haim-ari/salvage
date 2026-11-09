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
    sync::{atomic::AtomicBool, Arc},
    time::{SystemTime, UNIX_EPOCH},
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
}

type CmdResult<T> = Result<T, ApiError>;

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    account: Option<Account>,
    has_snapshot: bool,
    version: String,
}

#[tauri::command]
fn app_state(app: AppHandle, state: State<AppState>) -> AppInfo {
    AppInfo {
        account: state.icloud.account(),
        has_snapshot: state.snapshot.read().is_some(),
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
    if let Some(s) = state.store.snapshot(&account.dsid) {
        *state.snapshot.write() = Some(Arc::new(s));
    }
    Some(account)
}

#[tauri::command]
fn sign_out(app: AppHandle, state: State<AppState>) {
    state.icloud.sign_out();
    state.store.clear();
    *state.snapshot.write() = None;
    state.restored.lock().clear();
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.clear_all_browsing_data();
    }
    let _ = app.emit("auth://changed", Option::<Account>::None);
}

#[derive(Serialize)]
struct View<'a> {
    snapshot: &'a Snapshot,
    restored: &'a HashSet<String>,
}

fn view(state: &AppState) -> Response {
    let json = state
        .snapshot
        .read()
        .clone()
        .and_then(|snap| {
            let restored = state.restored.lock();
            serde_json::to_string(&View { snapshot: &snap, restored: &restored }).ok()
        })
        .unwrap_or_else(|| "null".into());
    Response::new(json)
}

#[tauri::command]
fn get_snapshot(state: State<AppState>) -> Response {
    // After a new sign-in the saved scan is not in memory yet: load it from disk.
    if state.snapshot.read().is_none() {
        if let Some(acc) = state.icloud.account() {
            if let Some(s) = state.store.snapshot(&acc.dsid) {
                *state.snapshot.write() = Some(Arc::new(s));
            }
        }
    }
    view(&state)
}

/// Turn Apple's folder hierarchy reply into path parts below iCloud Drive.
fn hierarchy_parts(entry: &Value) -> Option<Vec<String>> {
    let is_root = |v: &Value| v["drivewsid"].as_str().map(|d| d.ends_with("::root")).unwrap_or(false);
    let mut chain: Vec<&Value> = entry["hierarchy"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    if chain.last().map(|l| is_root(l)).unwrap_or(false) && !chain.first().map(|f| is_root(f)).unwrap_or(false) {
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
            Some(e) if !e.is_empty() && node["type"].as_str() != Some("FOLDER") => format!("{name}.{e}"),
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
                    let Some(id) = e["drivewsid"].as_str() else { continue };
                    let parts = if id.ends_with("::root") { Some(Vec::new()) } else { hierarchy_parts(&e) };
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
        Overview { files: b.items.len(), bytes, pages, complete, unknown: b.unplaced.len(), folders: &folders },
    );
}

#[tauri::command]
async fn scan(app: AppHandle, fresh: Option<bool>) -> CmdResult<Response> {
    let state = app.state::<AppState>();
    let account = state.icloud.account().ok_or(ApiError::NotSignedIn)?;
    let icloud: &Icloud = &state.icloud;
    // A finished scan is kept. Only an explicit "Scan again" (fresh) reads Apple's list again.
    if !fresh.unwrap_or(false) && state.store.scan_state().map(|s| s.complete && s.dsid == account.dsid).unwrap_or(false) {
        if let Some(snap) = state.store.snapshot(&account.dsid) {
            *state.snapshot.write() = Some(Arc::new(snap));
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
    let save = |r: &HashMap<String, Vec<String>>, t: &HashSet<String>| store.save_paths(&dsid, r, t);

    // Continue a scan that stopped (app closed, crash) instead of starting again.
    let saved = state
        .store
        .scan_state()
        .filter(|s| !fresh.unwrap_or(false) && s.dsid == account.dsid && !s.complete && s.pages > 0 && s.marker.is_some())
        .and_then(|s| state.store.scan_pages(s.pages).map(|p| (s, p)));
    let mut st = match saved {
        Some((s, pages)) => {
            for page in &pages {
                for d in page {
                    b.push(d);
                }
            }
            tracing::info!(target: "scan", pages = s.pages, items = b.items.len(), "resuming saved scan");
            let _ = app.emit("scan://progress", serde_json::json!({ "count": b.items.len(), "phase": "list" }));
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
            store::ScanState { dsid: account.dsid.clone(), ..Default::default() }
        }
    };
    let mut unverified = st.pages > 0;

    while !st.complete {
        let (docs, next) = match icloud.tombstone_page(st.marker.as_deref()).await {
            Ok(x) => x,
            Err(ApiError::AuthExpired) => return Err(ApiError::AuthExpired),
            Err(e) if unverified => {
                // Apple did not accept the saved marker: start the list again.
                tracing::warn!(target: "scan", error = %e, "saved marker refused, starting over");
                unverified = false;
                state.store.scan_reset(&account.dsid);
                st = store::ScanState { dsid: account.dsid.clone(), ..Default::default() };
                b = Builder::new();
                continue;
            }
            Err(e) => return Err(e),
        };
        unverified = false;
        if st.pages == 0 {
            state.store.save_sample(&docs);
        }
        for d in &docs {
            b.push(d);
        }
        st.pages += 1;
        st.complete = next.is_none();
        st.marker = next;
        state.store.scan_page(&st, &docs);
        let _ = app.emit("scan://progress", serde_json::json!({ "count": b.items.len(), "phase": "list" }));
        if resolve_paths(&app, icloud, &b, &mut resolved, &mut tried).await? {
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
    *state.snapshot.write() = Some(Arc::new(snap));
    Ok(view(&state))
}

/// Hide likely copies in the saved scan (no network).
#[tauri::command]
fn find_copies(state: State<AppState>) -> CmdResult<Response> {
    let snap = state.snapshot.read().clone().ok_or_else(|| ApiError::Shape("scan first".into()))?;
    let mut snap = (*snap).clone();
    copies::mark(&mut snap);
    state.store.save_snapshot(&snap);
    *state.snapshot.write() = Some(Arc::new(snap));
    Ok(view(&state))
}

#[derive(Serialize)]
struct IcloudFolder {
    id: String,
    name: String,
    count: u64,
}

#[tauri::command]
async fn list_icloud_folder(state: State<'_, AppState>, id: Option<String>) -> CmdResult<Vec<IcloudFolder>> {
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
                        count: x["directChildrenCount"].as_u64().or_else(|| x["numberOfItems"].as_u64()).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|f| f.name.to_lowercase());
    Ok(out)
}

#[tauri::command]
async fn create_icloud_folder(state: State<'_, AppState>, parent: String, name: String) -> CmdResult<String> {
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
fn start_restore(app: AppHandle, state: State<AppState>, ids: Vec<String>, destination: Destination) -> CmdResult<String> {
    let snap = state.snapshot.read().clone().ok_or_else(|| ApiError::Shape("scan first".into()))?;
    state.icloud.session()?;
    let mut job_slot = state.job.lock();
    if job_slot.as_ref().map(|c| !c.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(false) {
        return Err(ApiError::Shape("a restore is already running".into()));
    }
    let wanted: HashSet<String> = ids.into_iter().collect();
    // Hidden copies always come back with the file that is shown for them.
    let items: Vec<_> = snap.items.iter().filter(|i| wanted.contains(&i.i) || (!i.o.is_empty() && wanted.contains(&i.o))).cloned().collect();
    if items.is_empty() {
        return Err(ApiError::Shape("nothing selected".into()));
    }
    let cancel = Arc::new(AtomicBool::new(false));
    *job_slot = Some(cancel.clone());
    let id = uuid::Uuid::new_v4().to_string();
    let job = Job { id: id.clone(), items, folders: snap.folders.clone(), dest: destination, cancel: cancel.clone() };
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

#[tauri::command]
fn reveal(path: String) {
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
}

#[tauri::command]
fn open_folder(path: String) {
    let _ = std::process::Command::new("open").arg(path).spawn();
}

fn init_logs() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_env("SALVAGE_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    if std::env::var("SALVAGE_LOG_PRETTY").is_ok() {
        fmt().with_env_filter(filter).init();
    } else {
        fmt().json().with_current_span(false).with_env_filter(filter).init();
    }
}

fn main() {
    init_logs();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let store = Store::new(dir);
            let restored = store.restored();
            app.manage(AppState {
                icloud: Icloud::new(),
                store,
                snapshot: RwLock::new(None),
                restored: Mutex::new(restored),
                job: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_state,
            sign_in,
            sign_out,
            resume_session,
            get_snapshot,
            scan,
            find_copies,
            list_icloud_folder,
            create_icloud_folder,
            pick_local_folder,
            start_restore,
            cancel_restore,
            reveal,
            open_folder
        ])
        .run(tauri::generate_context!())
        .expect("error while running Salvage");
}
