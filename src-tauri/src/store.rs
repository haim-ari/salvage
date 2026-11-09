//! Local cache in the app data folder: last scan, restored ids, a raw sample for diagnostics.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    io::{BufRead, Write},
    path::PathBuf,
};

use crate::model::Snapshot;

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

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
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
        self.read::<Snapshot>("snapshot.json").filter(|s| s.dsid == dsid)
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

    /// Keeps the first raw records Apple returned, so field mapping can be checked offline.
    pub fn save_sample(&self, docs: &[serde_json::Value]) {
        let sample: Vec<_> = docs.iter().take(5).cloned().collect();
        self.write("tombstone-sample.json", &sample);
    }

    pub fn scan_state(&self) -> Option<ScanState> {
        self.read("scan-state.json")
    }

    /// Start a new scan: forget saved pages.
    pub fn scan_reset(&self, dsid: &str) {
        let _ = std::fs::remove_file(self.dir.join("tombstones.jsonl"));
        self.write("scan-state.json", &ScanState { dsid: dsid.into(), ..Default::default() });
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
    pub fn paths(&self, dsid: &str) -> Option<(HashMap<String, Vec<String>>, HashSet<String>)> {
        let c: PathCache = self.read("paths-cache.json")?;
        (c.dsid == dsid).then_some((c.resolved, c.tried))
    }

    pub fn save_paths(&self, dsid: &str, resolved: &HashMap<String, Vec<String>>, tried: &HashSet<String>) {
        self.write("paths-cache.json", &PathCacheRef { dsid, resolved, tried });
    }

    pub fn fingerprints(&self) -> HashMap<String, String> {
        self.read("fingerprints.json").unwrap_or_default()
    }

    pub fn save_fingerprints(&self, f: &HashMap<String, String>) {
        self.write("fingerprints.json", f);
    }

    /// docwsid -> Apple's download link, from the saved list pages.
    pub fn tombstone_urls(&self) -> HashMap<String, String> {
        let mut out = HashMap::new();
        let Ok(f) = std::fs::File::open(self.dir.join("tombstones.jsonl")) else { return out };
        for line in std::io::BufReader::new(f).lines().map_while(Result::ok) {
            let Ok(docs) = serde_json::from_str::<Vec<Value>>(&line) else { continue };
            for d in docs {
                if let (Some(di), Some(u)) = (d["docwsid"].as_str(), d["urls"]["url_download"].as_str()) {
                    out.insert(di.to_string(), u.to_string());
                }
            }
        }
        out
    }

    pub fn clear_paths(&self) {
        let _ = std::fs::remove_file(self.dir.join("paths-cache.json"));
    }

    pub fn clear(&self) {
        for f in ["snapshot.json", "restored.json", "tombstone-sample.json", "scan-state.json", "tombstones.jsonl", "paths-cache.json", "fingerprints.json"] {
            let _ = std::fs::remove_file(self.dir.join(f));
        }
    }
}
