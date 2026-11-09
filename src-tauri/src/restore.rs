//! The restore engine.
//!
//! Phase 1 always asks Apple to recover the items (puts them back where they were).
//! Phase 2 then places them: leave them (Original), move them into another iCloud
//! folder keeping the relative structure (Icloud), or download them to a Mac folder
//! with their dates and exec bit kept (Local).

use futures::{stream, StreamExt};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

use crate::api::{ApiError, DEFAULT_ZONE};
use crate::model::{Folder, Item};
use crate::AppState;

const RECOVER_BATCH: usize = 100;
const RECOVER_PAR: usize = 6;
const RECOVER_RETRIES: u32 = 6;

/// Apple errors that go away if we wait and ask again.
fn is_transient(msg: &str) -> bool {
    let m = msg.to_ascii_uppercase();
    [
        "OVERLOAD",
        "THROTTL",
        "TRY_AGAIN",
        "RETRY",
        "UNAVAILABLE",
        "TIMED OUT",
        "TIMEOUT",
        "503",
        "502",
        "504",
        "429",
        "STATUS 5",
    ]
    .iter()
    .any(|k| m.contains(k))
}
const MOVE_BATCH: usize = 200;
const DOWNLOAD_PAR: usize = 12;
const MOVE_PAR: usize = 4;

#[derive(Deserialize, Clone, Debug)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Destination {
    Original,
    Icloud {
        drivewsid: String,
        #[allow(dead_code)]
        path: String,
    },
    Local {
        path: String,
    },
}

#[derive(Serialize, Clone, Debug)]
pub struct Failure {
    pub id: String,
    pub name: String,
    pub error: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub job: String,
    pub phase: String,
    pub total: usize,
    pub recovered: usize,
    pub placed: usize,
    pub failed: usize,
    pub bytes: u64,
    pub total_bytes: u64,
    pub done: bool,
    pub cancelled: bool,
    pub error: Option<String>,
    pub failures: Vec<Failure>,
    /// Where the files ended up, for "Show in Finder" (Local only).
    pub output: Option<String>,
}

struct Reporter {
    app: AppHandle,
    p: Mutex<Progress>,
    last: Mutex<Instant>,
    bytes: AtomicU64,
    /// failures already sent to the window; the list is sent again only when it grows
    sent_failures: AtomicU64,
}

impl Reporter {
    fn update(&self, f: impl FnOnce(&mut Progress)) {
        f(&mut self.p.lock());
        self.emit(false);
    }

    fn fail(&self, it: &Item, error: String) {
        self.update(|p| {
            p.failed += 1;
            if p.failures.len() < 500 {
                p.failures.push(Failure {
                    id: it.i.clone(),
                    name: it.n.clone(),
                    error,
                });
            }
        });
    }

    fn emit(&self, force: bool) {
        let mut last = self.last.lock();
        if !force && last.elapsed() < Duration::from_millis(80) {
            return;
        }
        *last = Instant::now();
        let snap = {
            let mut p = self.p.lock();
            let failures = std::mem::take(&mut p.failures);
            let mut snap = p.clone();
            let n = failures.len() as u64;
            if self.sent_failures.swap(n, Ordering::Relaxed) != n || p.done {
                snap.failures = failures.clone();
            }
            p.failures = failures;
            snap
        };
        let snap = Progress {
            bytes: self.bytes.load(Ordering::Relaxed),
            ..snap
        };
        let _ = self.app.emit("restore://progress", snap);
    }
}

pub struct Job {
    pub id: String,
    pub items: Vec<Item>,
    pub folders: Vec<Folder>,
    pub dest: Destination,
    pub cancel: Arc<AtomicBool>,
}

fn components(path: &str) -> Vec<String> {
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Relative folder (as components) for each item under the destination.
/// All in one folder: flat. Otherwise keep the deepest common folder's name and below.
#[cfg(test)]
pub fn relative_dirs(items: &[Item], folders: &[Folder]) -> Vec<Vec<String>> {
    dirs_of(items.iter().map(|it| it.f), folders)
}

fn dirs_of(fs: impl Iterator<Item = u32>, folders: &[Folder]) -> Vec<Vec<String>> {
    let paths: Vec<Vec<String>> = fs
        .map(|f| {
            folders
                .get(f as usize)
                .map(|f| components(&f.path))
                .unwrap_or_default()
        })
        .collect();
    let Some(first) = paths.first() else {
        return Vec::new();
    };
    if paths.iter().all(|p| p == first) {
        return vec![Vec::new(); paths.len()];
    }
    let mut common = first.len();
    for p in &paths {
        common = common.min(p.iter().zip(first).take_while(|(a, b)| a == b).count());
    }
    let keep_from = common.saturating_sub(1);
    paths.into_iter().map(|p| p[keep_from..].to_vec()).collect()
}

/// Like `relative_dirs`, but a hidden copy goes in the same folder as the file shown for
/// it, so copies never change the folder layout of what the person picked.
fn rel_with_copies(items: &[Item], folders: &[Folder]) -> Vec<Vec<String>> {
    let shown: Vec<usize> = (0..items.len())
        .filter(|&i| items[i].o.is_empty())
        .collect();
    let rel_shown = dirs_of(shown.iter().map(|&i| items[i].f), folders);
    let by_id: HashMap<&str, Vec<String>> = shown
        .iter()
        .map(|&i| items[i].i.as_str())
        .zip(rel_shown)
        .collect();
    items
        .iter()
        .map(|it| {
            let key = if it.o.is_empty() {
                it.i.as_str()
            } else {
                it.o.as_str()
            };
            by_id.get(key).cloned().unwrap_or_default()
        })
        .collect()
}

fn is_auth(e: &ApiError) -> bool {
    matches!(e, ApiError::AuthExpired | ApiError::NotSignedIn)
}

pub async fn run(app: AppHandle, job: Job) {
    let total_bytes = job.items.iter().map(|i| i.s).sum();
    let rep = Arc::new(Reporter {
        app: app.clone(),
        p: Mutex::new(Progress {
            job: job.id.clone(),
            phase: "recover".into(),
            total: job.items.len(),
            total_bytes,
            ..Default::default()
        }),
        last: Mutex::new(Instant::now() - Duration::from_secs(1)),
        bytes: AtomicU64::new(0),
        sent_failures: AtomicU64::new(0),
    });
    rep.emit(true);
    let started = Instant::now();
    let result = execute(&app, &job, &rep).await;
    rep.update(|p| {
        p.done = true;
        p.cancelled = job.cancel.load(Ordering::Relaxed);
        if let Err(e) = &result {
            p.error = Some(e.to_string());
        }
    });
    rep.emit(true);
    let p = rep.p.lock().clone();
    tracing::info!(target: "restore", job = %job.id, total = p.total, recovered = p.recovered, placed = p.placed,
        failed = p.failed, cancelled = p.cancelled, secs = started.elapsed().as_secs_f32(), "restore finished");
}

async fn execute(app: &AppHandle, job: &Job, rep: &Arc<Reporter>) -> Result<(), ApiError> {
    let state = app.state::<AppState>();

    // ---- Phase 1: recover ----
    let (done_before, todo): (Vec<&Item>, Vec<&Item>) = {
        let already = state.restored.lock();
        job.items.iter().partition(|i| already.contains(&i.i))
    };
    rep.update(|p| p.recovered = done_before.len());
    let by_id: HashMap<&str, &Item> = job.items.iter().map(|i| (i.i.as_str(), i)).collect();
    let mut ok: HashSet<String> = done_before.iter().map(|i| i.i.clone()).collect();
    let auth_lost = AtomicBool::new(false);

    // Apple sometimes answers SERVER_OVERLOADED or a 5xx for part of a batch.
    // Those items go into the next round, after a pause, with less parallelism.
    let mut pending: Vec<String> = todo.iter().map(|i| i.i.clone()).collect();
    for round in 0..=RECOVER_RETRIES {
        if pending.is_empty() || job.cancel.load(Ordering::Relaxed) {
            break;
        }
        if round > 0 {
            let wait = 5u64 << (round - 1).min(4);
            tracing::warn!(target: "restore", job = %job.id, round, count = pending.len(), wait, "apple busy, retrying");
            for _ in 0..wait {
                if job.cancel.load(Ordering::Relaxed) {
                    break;
                }
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        }
        let last = round == RECOVER_RETRIES;
        let par = if round == 0 { RECOVER_PAR } else { 2 };
        let batches: Vec<Vec<String>> = pending.chunks(RECOVER_BATCH).map(|c| c.to_vec()).collect();
        let mut again: Vec<String> = Vec::new();
        let mut results = stream::iter(batches)
            .map(|ids| {
                let state = app.state::<AppState>();
                let cancel = job.cancel.clone();
                async move {
                    if cancel.load(Ordering::Relaxed) {
                        return (ids, Err(ApiError::Cancelled));
                    }
                    let r = state.icloud.recover(&ids).await;
                    (ids, r)
                }
            })
            .buffer_unordered(par);
        while let Some((ids, r)) = results.next().await {
            match r {
                Ok(failed) => {
                    let bad: HashMap<String, String> = failed.into_iter().collect();
                    for id in &ids {
                        match bad.get(id) {
                            Some(msg) if !last && is_transient(msg) => again.push(id.clone()),
                            Some(msg) => rep.fail(by_id[id.as_str()], msg.clone()),
                            None => {
                                ok.insert(id.clone());
                                rep.update(|p| p.recovered += 1);
                            }
                        }
                    }
                }
                Err(ApiError::Cancelled) => {}
                Err(e) => {
                    if is_auth(&e) {
                        auth_lost.store(true, Ordering::Relaxed);
                        job.cancel.store(true, Ordering::Relaxed);
                    }
                    let msg = e.to_string();
                    if !last && !is_auth(&e) && is_transient(&msg) {
                        again.extend(ids);
                    } else {
                        for id in &ids {
                            rep.fail(by_id[id.as_str()], msg.clone());
                        }
                    }
                }
            }
        }
        drop(results);
        pending = again;
    }
    // Cancelled while items waited for a retry: they did not come back.
    for id in &pending {
        rep.fail(by_id[id.as_str()], "stopped before retry".into());
    }

    // Remember what came back, so a second run skips it and the list can mark it.
    {
        let mut set = state.restored.lock();
        set.extend(ok.iter().cloned());
        state.store.save_restored(&set);
    }
    if auth_lost.load(Ordering::Relaxed) {
        return Err(ApiError::AuthExpired);
    }
    if job.cancel.load(Ordering::Relaxed) {
        return Ok(());
    }

    let placed: Vec<(usize, &Item)> = job
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| ok.contains(&i.i))
        .collect();
    let rel = rel_with_copies(&job.items, &job.folders);

    match &job.dest {
        Destination::Original => {
            rep.update(|p| {
                p.placed = placed.len();
                p.bytes = 0;
            });
            rep.bytes
                .store(placed.iter().map(|(_, i)| i.s).sum(), Ordering::Relaxed);
            Ok(())
        }
        Destination::Icloud { drivewsid, .. } => {
            rep.update(|p| p.phase = "move".into());
            // Hidden copies stay where they were in iCloud; only the shown files move.
            let placed: Vec<(usize, &Item)> =
                placed.into_iter().filter(|(_, i)| i.o.is_empty()).collect();
            place_icloud(app, job, rep, drivewsid, &placed, &rel).await
        }
        Destination::Local { path } => {
            rep.update(|p| {
                p.phase = "download".into();
                p.output = Some(path.clone());
            });
            place_local(app, job, rep, Path::new(path), &placed, &rel).await
        }
    }
}

// ---------------- iCloud folder ----------------

fn child_folders(entry: &Value) -> Vec<(String, String)> {
    entry["items"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|x| matches!(x["type"].as_str(), Some("FOLDER") | Some("APP_LIBRARY")))
                .filter_map(|x| {
                    Some((
                        x["name"].as_str()?.to_string(),
                        x["drivewsid"].as_str()?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn ensure_dir(
    app: &AppHandle,
    cache: &mut HashMap<Vec<String>, String>,
    listed: &mut HashMap<String, Vec<(String, String)>>,
    root: &str,
    rel: &[String],
) -> Result<String, ApiError> {
    let state = app.state::<AppState>();
    let mut parent = root.to_string();
    for depth in 1..=rel.len() {
        let key = rel[..depth].to_vec();
        if let Some(id) = cache.get(&key) {
            parent = id.clone();
            continue;
        }
        let name = &rel[depth - 1];
        if !listed.contains_key(&parent) {
            let entries = state.icloud.folders(&[parent.clone()], false).await?;
            listed.insert(
                parent.clone(),
                entries.first().map(child_folders).unwrap_or_default(),
            );
        }
        let existing = listed[&parent]
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, id)| id.clone());
        let id = match existing {
            Some(id) => id,
            None => {
                let id = state.icloud.create_folder(&parent, name).await?;
                listed
                    .get_mut(&parent)
                    .expect("listed")
                    .push((name.clone(), id.clone()));
                listed.insert(id.clone(), Vec::new());
                id
            }
        };
        cache.insert(key, id.clone());
        parent = id;
    }
    Ok(parent)
}

fn file_drivewsid(it: &Item) -> String {
    let zone = if it.z.is_empty() { DEFAULT_ZONE } else { &it.z };
    format!("FILE::{zone}::{}", it.di)
}

async fn place_icloud(
    app: &AppHandle,
    job: &Job,
    rep: &Arc<Reporter>,
    root: &str,
    placed: &[(usize, &Item)],
    rel: &[Vec<String>],
) -> Result<(), ApiError> {
    let mut groups: HashMap<Vec<String>, Vec<&Item>> = HashMap::new();
    for (idx, it) in placed {
        groups.entry(rel[*idx].clone()).or_default().push(it);
    }
    let mut keys: Vec<_> = groups.keys().cloned().collect();
    keys.sort_by_key(|k| (k.len(), k.clone()));

    // Make the folders one by one (a parent before its children), then move files in parallel.
    let mut cache = HashMap::new();
    let mut listed = HashMap::new();
    let mut moves: Vec<(String, &[&Item])> = Vec::new();
    for key in &keys {
        if job.cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let items = &groups[key];
        match ensure_dir(app, &mut cache, &mut listed, root, key).await {
            Ok(dir) => moves.extend(items.chunks(MOVE_BATCH).map(|c| (dir.clone(), c))),
            Err(e) if is_auth(&e) => return Err(e),
            Err(e) => {
                for it in items {
                    rep.fail(it, format!("could not make folder {}: {e}", key.join("/")));
                }
            }
        }
    }
    let jobs: Vec<_> = moves
        .into_iter()
        .map(|(dir, chunk)| Box::pin(move_chunk(app, job, rep, dir, chunk)))
        .collect();
    let mut results = stream::iter(jobs).buffer_unordered(MOVE_PAR);
    while let Some(r) = results.next().await {
        r?;
    }
    Ok(())
}

async fn move_chunk(
    app: &AppHandle,
    job: &Job,
    rep: &Arc<Reporter>,
    dir: String,
    chunk: &[&Item],
) -> Result<(), ApiError> {
    if job.cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    let state = app.state::<AppState>();
    let by_ws: HashMap<String, &Item> = chunk.iter().map(|it| (file_drivewsid(it), *it)).collect();
    let mut pending: Vec<(String, String)> =
        by_ws.keys().map(|k| (k.clone(), String::new())).collect();
    // Recovered items can take a moment to show up in drivews: retry what failed.
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2 * attempt)).await;
        }
        let failed = match state.icloud.move_items(&dir, &pending).await {
            Ok(f) => f,
            Err(e) if is_auth(&e) => return Err(e),
            Err(e) => pending
                .iter()
                .map(|(id, _)| (id.clone(), e.to_string()))
                .collect(),
        };
        let failed_ids: HashSet<&String> = failed.iter().map(|(id, _)| id).collect();
        let moved: Vec<&Item> = pending
            .iter()
            .filter(|(id, _)| !failed_ids.contains(id))
            .filter_map(|(id, _)| by_ws.get(id).copied())
            .collect();
        rep.bytes
            .fetch_add(moved.iter().map(|i| i.s).sum(), Ordering::Relaxed);
        rep.update(|p| p.placed += moved.len());
        if failed.is_empty() {
            return Ok(());
        }
        if attempt == 2 {
            for (id, msg) in &failed {
                if let Some(it) = by_ws.get(id) {
                    rep.fail(it, format!("restored to original place, not moved: {msg}"));
                }
            }
        }
        pending = failed
            .into_iter()
            .map(|(id, _)| (id, String::new()))
            .collect();
    }
    Ok(())
}

// ---------------- Mac folder ----------------

fn unique_path(dir: &Path, name: &str, taken: &Mutex<HashSet<PathBuf>>) -> PathBuf {
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    let mut taken = taken.lock();
    let mut n = 1;
    loop {
        let candidate = if n == 1 {
            dir.join(name)
        } else {
            dir.join(format!("{stem} {n}{ext}"))
        };
        if !candidate.exists() && !taken.contains(&candidate) {
            taken.insert(candidate.clone());
            return candidate;
        }
        n += 1;
    }
}

fn safe_component(s: &str) -> String {
    let c: String = s
        .chars()
        .map(|c| if c == '/' || c == '\0' { '_' } else { c })
        .collect();
    if c == "." || c == ".." || c.is_empty() {
        "_".into()
    } else {
        c
    }
}

fn ms_to_time(ms: i64) -> Option<SystemTime> {
    (ms > 0).then(|| UNIX_EPOCH + Duration::from_millis(ms as u64))
}

fn set_meta(path: &Path, it: &Item) -> std::io::Result<()> {
    let f = std::fs::File::options().write(true).open(path)?;
    let mut times = std::fs::FileTimes::new();
    if let Some(m) = ms_to_time(it.m) {
        times = times.set_modified(m).set_accessed(m);
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::FileTimesExt;
        if let Some(c) = ms_to_time(it.c) {
            times = times.set_created(c);
        }
    }
    f.set_times(times)?;
    #[cfg(unix)]
    if it.x {
        use std::os::unix::fs::PermissionsExt;
        let mut perm = f.metadata()?.permissions();
        perm.set_mode(perm.mode() | 0o111);
        std::fs::set_permissions(path, perm)?;
    }
    Ok(())
}

/// Download one file. Returns the SHA-256 of its bytes.
async fn download_one(
    app: &AppHandle,
    rep: &Reporter,
    it: &Item,
    target: &Path,
    cancel: &AtomicBool,
) -> Result<[u8; 32], ApiError> {
    let state = app.state::<AppState>();
    let dir = target.parent().expect("parent");
    let tmp = dir.join(format!(".salvage-{}.part", uuid::Uuid::new_v4()));
    let mut last_err = ApiError::Shape("download failed".into());
    for attempt in 0..5u32 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(500 * 2u64.pow(attempt - 1))).await;
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(ApiError::Cancelled);
        }
        let attempt_result: Result<[u8; 32], ApiError> = async {
            let url = state.icloud.download_url(&it.z, &it.di).await?;
            let resp = state.icloud.download(&url).await?;
            let mut file =
                tokio::io::BufWriter::with_capacity(1 << 20, tokio::fs::File::create(&tmp).await?);
            let mut body = resp.bytes_stream();
            let mut h = Sha256::new();
            while let Some(chunk) = body.next().await {
                if cancel.load(Ordering::Relaxed) {
                    return Err(ApiError::Cancelled);
                }
                let chunk = chunk?;
                file.write_all(&chunk).await?;
                h.update(&chunk);
                rep.bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            }
            file.flush().await?;
            Ok(h.finalize().into())
        }
        .await;
        match attempt_result {
            Ok(hash) => {
                tokio::fs::rename(&tmp, target).await?;
                if let Err(e) = set_meta(target, it) {
                    tracing::warn!(target: "restore", error = %e, "could not set file dates");
                }
                return Ok(hash);
            }
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                if is_auth(&e) || matches!(e, ApiError::Cancelled | ApiError::Io(_)) {
                    return Err(e);
                }
                last_err = e;
            }
        }
    }
    Err(last_err)
}

async fn place_local(
    app: &AppHandle,
    job: &Job,
    rep: &Arc<Reporter>,
    root: &Path,
    placed: &[(usize, &Item)],
    rel: &[Vec<String>],
) -> Result<(), ApiError> {
    let taken = Mutex::new(HashSet::new());
    let auth_lost = AtomicBool::new(false);
    let mut targets = Vec::with_capacity(placed.len());
    // Shown files first, so they get the plain name and a copy gets "name 2".
    let mut ordered: Vec<&(usize, &Item)> = placed.iter().collect();
    ordered.sort_by_key(|(_, i)| !i.o.is_empty());
    for (idx, it) in ordered {
        let mut dir = root.to_path_buf();
        for c in &rel[*idx] {
            dir.push(safe_component(c));
        }
        if let Err(e) = std::fs::create_dir_all(&dir) {
            rep.fail(it, format!("could not make folder: {e}"));
            continue;
        }
        targets.push((*it, unique_path(&dir, &safe_component(&it.n), &taken)));
    }
    let hashes: Mutex<HashMap<String, ([u8; 32], PathBuf)>> = Mutex::new(HashMap::new());
    stream::iter(targets)
        .for_each_concurrent(DOWNLOAD_PAR, |(it, target)| {
            let rep = rep.clone();
            let auth_lost = &auth_lost;
            let hashes = &hashes;
            async move {
                if job.cancel.load(Ordering::Relaxed) {
                    return;
                }
                match download_one(app, &rep, it, &target, &job.cancel).await {
                    Ok(h) => {
                        hashes.lock().insert(it.i.clone(), (h, target.clone()));
                        rep.update(|p| p.placed += 1)
                    }
                    Err(ApiError::Cancelled) => {}
                    Err(e) => {
                        if is_auth(&e) {
                            auth_lost.store(true, Ordering::Relaxed);
                            job.cancel.store(true, Ordering::Relaxed);
                        }
                        rep.fail(it, format!("restored in iCloud, download failed: {e}"));
                    }
                }
            }
        })
        .await;
    // A hidden copy with the same bytes as the file shown for it is removed: one file stays.
    // A copy with different bytes is kept next to it ("name 2.ext"), so nothing is lost.
    let hashes = hashes.into_inner();
    let (mut same, mut different) = (0usize, 0usize);
    for (_, it) in placed.iter().filter(|(_, i)| !i.o.is_empty()) {
        let (Some((hc, path)), Some((hk, _))) = (hashes.get(&it.i), hashes.get(&it.o)) else {
            continue;
        };
        if hc == hk {
            if std::fs::remove_file(path).is_ok() {
                same += 1;
            }
        } else {
            different += 1;
        }
    }
    if same + different > 0 {
        tracing::info!(target: "restore", same, different, "copies compared by bytes");
    }
    if auth_lost.load(Ordering::Relaxed) {
        return Err(ApiError::AuthExpired);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(f: u32) -> Item {
        Item {
            i: "i".into(),
            di: "d".into(),
            z: String::new(),
            p: String::new(),
            n: "a.py".into(),
            e: "py".into(),
            s: 1,
            d: 0,
            m: 0,
            c: 0,
            f,
            x: false,
            t: String::new(),
            u: false,
            o: String::new(),
        }
    }

    fn folders() -> Vec<Folder> {
        vec![
            Folder {
                name: "iCloud Drive".into(),
                parent: -1,
                path: String::new(),
            },
            Folder {
                name: "Code".into(),
                parent: 0,
                path: "Code".into(),
            },
            Folder {
                name: "proj".into(),
                parent: 1,
                path: "Code/proj".into(),
            },
            Folder {
                name: "src".into(),
                parent: 2,
                path: "Code/proj/src".into(),
            },
        ]
    }

    #[test]
    fn same_folder_is_flat() {
        let r = relative_dirs(&[item(3), item(3)], &folders());
        assert!(r.iter().all(|p| p.is_empty()));
    }

    #[test]
    fn keeps_common_folder_name() {
        let r = relative_dirs(&[item(2), item(3)], &folders());
        assert_eq!(r[0], vec!["proj"]);
        assert_eq!(r[1], vec!["proj", "src"]);
    }

    #[test]
    fn destination_json() {
        let d: Destination =
            serde_json::from_str(r#"{"kind":"icloud","drivewsid":"X","path":"/a"}"#).unwrap();
        assert!(matches!(d, Destination::Icloud { .. }));
        let d: Destination = serde_json::from_str(r#"{"kind":"original"}"#).unwrap();
        assert!(matches!(d, Destination::Original));
    }

    #[test]
    fn copies_follow_the_shown_file() {
        let mut a = item(3);
        a.i = "a".into();
        let mut b = item(1);
        b.i = "b".into();
        let mut c = item(2);
        c.i = "c".into();
        c.o = "a".into();
        let rel = rel_with_copies(&[a, b, c], &folders());
        assert_eq!(rel[2], rel[0]);
        assert_eq!(rel[0], vec!["Code", "proj", "src"]);
    }

    #[test]
    fn marks_copies_once() {
        let mut snap = crate::model::Snapshot {
            folders: folders(),
            ..Default::default()
        };
        let mut a = item(3);
        a.i = "a".into();
        a.m = 5;
        let mut b = item(1);
        b.i = "b".into();
        b.m = 5;
        let mut c = item(2);
        c.i = "c".into();
        c.m = 6;
        snap.items = vec![a, b, c];
        assert_eq!(crate::copies::mark(&mut snap), 1);
        assert!(snap.items[0].u && snap.items[0].o == "b");
        assert!(!snap.items[1].u && !snap.items[2].u);
    }

    #[test]
    fn unique_names() {
        let dir = std::env::temp_dir().join(format!("salvage-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.py"), b"x").unwrap();
        let taken = Mutex::new(HashSet::new());
        assert_eq!(unique_path(&dir, "a.py", &taken), dir.join("a 2.py"));
        assert_eq!(unique_path(&dir, "a.py", &taken), dir.join("a 3.py"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
