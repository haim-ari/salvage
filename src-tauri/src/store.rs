//! Local cache in the app data folder: last scan, restored ids, scan pages, folder paths.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, Write},
    path::{Path, PathBuf},
};

use crate::model::Snapshot;

/// Live folder id -> path, and every id Apple already answered.
pub type Paths = (HashMap<String, Vec<String>>, HashSet<String>);

/// Where a scan of Apple's list stopped. Pages already read are in `tombstones.jsonl`.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ScanState {
    pub dsid: String,
    /// marker for the next page; None when the list is complete
    pub marker: Option<String>,
    /// number of pages saved in tombstones.jsonl
    pub pages: usize,
    pub complete: bool,
}

#[derive(Deserialize)]
struct PathCache {
    dsid: String,
    resolved: HashMap<String, Vec<String>>,
    tried: HashSet<String>,
}

#[derive(Serialize)]
struct PathCacheRef<'a> {
    dsid: &'a str,
    resolved: &'a HashMap<String, Vec<String>>,
    tried: &'a HashSet<String>,
}

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        // The cache holds file names of the Apple Account: only this user may read it.
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        }
        migrate(&dir);
        Self { dir }
    }

    fn read<T: DeserializeOwned>(&self, name: &str) -> Option<T> {
        let bytes = std::fs::read(self.dir.join(name)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Write atomically: temp file then rename, so a crash never leaves half a file.
    fn write<T: Serialize>(&self, name: &str, value: &T) {
        let tmp = self.dir.join(format!(".{name}.tmp"));
        if let Ok(bytes) = serde_json::to_vec(value) {
            if std::fs::write(&tmp, bytes).is_ok() {
                let _ = std::fs::rename(&tmp, self.dir.join(name));
            }
        }
    }

    pub fn snapshot(&self, dsid: &str) -> Option<Snapshot> {
        self.last_snapshot().filter(|s| s.dsid == dsid)
    }

    /// The last saved scan, of any Apple Account.
    pub fn last_snapshot(&self) -> Option<Snapshot> {
        self.read::<Snapshot>("snapshot.json")
    }

    pub fn save_snapshot(&self, s: &Snapshot) {
        self.write("snapshot.json", s);
    }

    pub fn restored(&self) -> HashSet<String> {
        self.read("restored.json").unwrap_or_default()
    }

    pub fn save_restored(&self, ids: &HashSet<String>) {
        self.write("restored.json", ids);
    }

    pub fn scan_state(&self) -> Option<ScanState> {
        self.read("scan-state.json")
    }

    /// Start a new scan: forget saved pages.
    pub fn scan_reset(&self, dsid: &str) {
        let _ = std::fs::remove_file(self.dir.join("tombstones.jsonl"));
        self.write(
            "scan-state.json",
            &ScanState {
                dsid: dsid.into(),
                ..Default::default()
            },
        );
    }

    /// Save one page, then the marker. A crash in between only means a page is read twice.
    pub fn scan_page(&self, state: &ScanState, docs: &[Value]) {
        let ok = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.dir.join("tombstones.jsonl"))
            .and_then(|mut f| {
                let mut line = serde_json::to_vec(docs).unwrap_or_else(|_| b"[]".to_vec());
                line.push(b'\n');
                f.write_all(&line)
            })
            .is_ok();
        if ok {
            self.write("scan-state.json", state);
        }
    }

    /// Pages saved so far (at most `pages` lines, so a half-written last line is skipped).
    pub fn scan_pages(&self, pages: usize) -> Option<Vec<Vec<Value>>> {
        let f = std::fs::File::open(self.dir.join("tombstones.jsonl")).ok()?;
        let mut out = Vec::new();
        for line in std::io::BufReader::new(f).lines().take(pages) {
            out.push(serde_json::from_str(&line.ok()?).ok()?);
        }
        (out.len() == pages).then_some(out)
    }

    /// Folder lookups already done: live id -> path, and every id Apple already answered.
    pub fn paths(&self, dsid: &str) -> Option<Paths> {
        let c: PathCache = self.read("paths-cache.json")?;
        (c.dsid == dsid).then_some((c.resolved, c.tried))
    }

    pub fn save_paths(
        &self,
        dsid: &str,
        resolved: &HashMap<String, Vec<String>>,
        tried: &HashSet<String>,
    ) {
        self.write(
            "paths-cache.json",
            &PathCacheRef {
                dsid,
                resolved,
                tried,
            },
        );
    }

    pub fn clear_paths(&self) {
        let _ = std::fs::remove_file(self.dir.join("paths-cache.json"));
    }

    pub fn clear(&self) {
        for f in FILES {
            let _ = std::fs::remove_file(self.dir.join(f));
        }
    }
}

/// Every file the store keeps.
const FILES: [&str; 5] = [
    "snapshot.json",
    "restored.json",
    "scan-state.json",
    "tombstones.jsonl",
    "paths-cache.json",
];
/// The data folder of versions before 0.1.1, next to the current one.
const OLD_DIR: &str = "com.haimari.salvage";

/// Versions before 0.1.1 kept data under another app id. Move it once, so the last scan and
/// the list of restored files are not lost. Also delete files that no version reads any more.
fn migrate(dir: &Path) {
    for f in ["tombstone-sample.json", "fingerprints.json"] {
        let _ = std::fs::remove_file(dir.join(f));
    }
    let Some(old) = dir.parent().map(|p| p.join(OLD_DIR)) else {
        return;
    };
    if old == dir || !old.is_dir() || FILES.iter().any(|f| dir.join(f).exists()) {
        return;
    }
    for f in FILES {
        let (from, to) = (old.join(f), dir.join(f));
        if from.exists()
            && std::fs::rename(&from, &to).is_err()
            && std::fs::copy(&from, &to).is_ok()
        {
            let _ = std::fs::remove_file(&from);
        }
    }
    for f in ["tombstone-sample.json", "fingerprints.json"] {
        let _ = std::fs::remove_file(old.join(f));
    }
    let _ = std::fs::remove_dir(&old);
    tracing::info!(target: "store", "moved data from the old app folder");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_old_data_once() {
        let root = std::env::temp_dir().join(format!("salvage-{}", uuid::Uuid::new_v4()));
        let old = root.join(OLD_DIR);
        let new = root.join("io.github.haim-ari.salvage");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("restored.json"), r#"["a"]"#).unwrap();
        std::fs::write(old.join("tombstone-sample.json"), "[]").unwrap();
        let s = Store::new(new.clone());
        assert!(s.restored().contains("a"));
        assert!(!old.exists());
        // A second start with data in place does not touch anything.
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("restored.json"), r#"["b"]"#).unwrap();
        let s = Store::new(new);
        assert!(!s.restored().contains("b"));
        let _ = std::fs::remove_dir_all(root);
    }
}
