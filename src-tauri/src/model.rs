//! Turns Apple's raw tombstone records into a compact, indexed snapshot:
//! a flat list of files plus a folder tree rebuilt from each file's original path.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

use crate::api::DEFAULT_ZONE;

/// One deleted file. Short field names keep the IPC payload small for 100k+ items.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Item {
    /// item_id, used by the recover call
    pub i: String,
    /// document_id, used by downloads and drivews ids
    pub di: String,
    /// zone (app container)
    pub z: String,
    /// parent folder id at the time of delete
    pub p: String,
    /// full file name with extension
    pub n: String,
    /// lowercased extension
    pub e: String,
    /// size in bytes
    pub s: u64,
    /// deleted at, unix ms (0 = unknown)
    pub d: i64,
    /// modified at, unix ms
    pub m: i64,
    /// created at, unix ms
    pub c: i64,
    /// folder index into Snapshot::folders
    pub f: u32,
    /// executable bit
    #[serde(default)]
    pub x: bool,
    /// etag at delete time
    #[serde(default)]
    pub t: String,
    /// true when this file is an exact copy of another file in the list (same name and content)
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub u: bool,    /// for a hidden copy: item_id of the file that is shown instead
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub o: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Folder {
    pub name: String,
    /// parent index, -1 for the root
    pub parent: i32,
    /// "/"-joined path, "" for the root
    pub path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub scanned_at: i64,
    pub dsid: String,
    pub items: Vec<Item>,
    pub folders: Vec<Folder>,
    /// true when some paths could not be read from Apple's data
    pub partial_paths: bool,
    /// true once exact copies were looked for and marked
    #[serde(default)]
    pub copies_checked: bool,
}

fn st(v: &Value, keys: &[&str]) -> String {
    for k in keys {
        match v.get(*k) {
            Some(Value::String(s)) if !s.is_empty() => return s.clone(),
            Some(Value::Number(n)) => return n.to_string(),
            _ => {}
        }
    }
    String::new()
}

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

/// Accepts seconds, milliseconds, microseconds or an ISO-8601 string.
fn time_ms(v: &Value, keys: &[&str]) -> i64 {
    for k in keys {
        let Some(x) = v.get(*k) else { continue };
        if let Some(n) = num(x) {
            if n <= 0.0 {
                continue;
            }
            return if n < 1e11 { (n * 1000.0) as i64 } else if n > 1e14 { (n / 1000.0) as i64 } else { n as i64 };
        }
        if let Value::String(s) = x {
            if let Some(ms) = parse_iso(s) {
                return ms;
            }
        }
    }
    0
}

/// Minimal RFC 3339 parser (YYYY-MM-DDTHH:MM:SS[.fff](Z|±HH:MM)).
fn parse_iso(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[10] != b'T' {
        return None;
    }
    let p = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (p(0..4)?, p(5..7)?, p(8..10)?, p(11..13)?, p(14..16)?, p(17..19)?);
    // days from civil (Howard Hinnant)
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (mo + if mo > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let mut ms = ((days * 24 + h) * 60 + mi) * 60_000 + se * 1000;
    let rest = &s[19..];
    let mut tz = rest;
    if let Some(frac) = rest.strip_prefix('.') {
        let digits: String = frac.chars().take_while(|c| c.is_ascii_digit()).collect();
        let f3: String = format!("{digits:0<3}").chars().take(3).collect();
        ms += f3.parse::<i64>().unwrap_or(0);
        tz = &frac[digits.len()..];
    }
    if tz.len() >= 6 && (tz.starts_with('+') || tz.starts_with('-')) {
        let sign = if tz.starts_with('-') { -1 } else { 1 };
        let oh: i64 = tz[1..3].parse().ok()?;
        let om: i64 = tz[4..6].parse().ok()?;
        ms -= sign * (oh * 60 + om) * 60_000;
    }
    Some(ms)
}

/// Original folder path of a tombstone, as path components (file name excluded).
fn restore_path(v: &Value, file_name: &str) -> Option<Vec<String>> {
    let raw = v.get("restorePath").or_else(|| v.get("restore_path")).or_else(|| v.get("path"))?;
    let mut parts: Vec<String> = match raw {
        Value::String(s) => s.split('/').map(str::to_string).collect(),
        Value::Array(a) => a
            .iter()
            .filter_map(|x| match x {
                Value::String(s) => Some(s.clone()),
                Value::Object(_) => x.get("name").and_then(Value::as_str).map(str::to_string),
                _ => None,
            })
            .collect(),
        Value::Object(_) => return restore_path(raw, file_name),
        _ => return None,
    };
    parts.retain(|p| !p.is_empty());
    if parts.last().map(|l| l == file_name).unwrap_or(false) {
        parts.pop();
    }
    if parts.first().map(|f| f == "root" || f == "iCloud Drive").unwrap_or(false) {
        parts.remove(0);
    }
    Some(parts)
}

pub fn zone_label(zone: &str) -> Option<String> {
    if zone.is_empty() || zone == DEFAULT_ZONE {
        return None;
    }
    // "com.apple.Pages" / "iCloud.md.obsidian" -> "Pages" / "obsidian"
    let last = zone.rsplit(['.', '~']).next().unwrap_or(zone);
    Some(format!("{last} (app)"))
}

/// Name of the folder for files whose original place is not known.
pub const UNKNOWN: &str = "Folder unknown";
/// Parent of one group per deleted folder whose name Apple no longer gives.
pub const GROUPS: &str = "Deleted folders";

/// Stable name for a deleted folder: "Deleted folder 96A430B9".
fn group_name(id: &str) -> String {
    let tail = id.rsplit("::").next().unwrap_or(id);
    format!("Deleted folder {}", tail.chars().take(8).collect::<String>())
}

/// Totals for one folder, its subfolders included.
#[derive(Serialize, Clone, Debug)]
pub struct FolderStat {
    pub name: String,
    pub parent: i32,
    pub files: u64,
    pub bytes: u64,
    /// a few file names, only for groups of a deleted folder
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hint: Vec<String>,
}

pub struct Builder {
    folders: Vec<Folder>,
    index: HashMap<String, u32>,
    pub items: Vec<Item>,
    /// Path parts read from the record itself (rare), parallel to `items`.
    fixed: Vec<Option<Vec<String>>>,
    seen: HashSet<String>,
    pub unplaced: Vec<usize>,
    /// Deleted folders from the tombstone list: drivewsid -> (name, parent drivewsid).
    pub dead_folders: HashMap<String, (String, String)>,
}

impl Builder {
    /// Walk up through deleted folders. Returns (first live ancestor id, names below it, top first).
    pub fn dead_chain(&self, parent: &str) -> (String, Vec<String>) {
        let mut names = Vec::new();
        let mut cur = parent.to_string();
        let mut guard = 0;
        while let Some((name, up)) = self.dead_folders.get(&cur) {
            names.push(name.clone());
            guard += 1;
            if up.is_empty() || guard > 200 {
                cur = String::new();
                break;
            }
            cur = up.clone();
        }
        names.reverse();
        (cur, names)
    }

    pub fn new() -> Self {
        let mut b = Self {
            folders: Vec::new(),
            index: HashMap::new(),
            items: Vec::new(),
            fixed: Vec::new(),
            seen: HashSet::new(),
            unplaced: Vec::new(),
            dead_folders: HashMap::new(),
        };
        b.reset_folders();
        b
    }

    fn reset_folders(&mut self) {
        self.folders.clear();
        self.index.clear();
        self.folders.push(Folder { name: "iCloud Drive".into(), parent: -1, path: String::new() });
        self.index.insert(String::new(), 0);
    }

    /// Full drivews id of the item's parent folder, or "" when the record has none.
    pub fn parent_key(it: &Item) -> String {
        if it.p.is_empty() {
            String::new()
        } else if it.p.contains("::") {
            it.p.clone()
        } else {
            let zone = if it.z.is_empty() { DEFAULT_ZONE } else { &it.z };
            format!("FOLDER::{zone}::{}", it.p)
        }
    }

    /// Live folder ids that unplaced items need and that are not in `known` yet.
    pub fn wanted(&self, known: &HashSet<String>) -> Vec<String> {
        let mut out = HashSet::new();
        for (i, it) in self.items.iter().enumerate() {
            if self.fixed[i].is_some() {
                continue;
            }
            let key = Self::parent_key(it);
            if key.is_empty() {
                continue;
            }
            let (live, _) = self.dead_chain(&key);
            if !live.is_empty() && !known.contains(&live) {
                out.insert(live);
            }
        }
        out.into_iter().collect()
    }

    /// Place every item in the folder tree again, from scratch. Deleted parent folders can
    /// arrive in a later page than their files, so this runs after each page.
    /// `resolved` maps a live folder id to its path below iCloud Drive.
    pub fn relayout(&mut self, resolved: &HashMap<String, Vec<String>>) {
        self.reset_folders();
        self.unplaced.clear();
        for i in 0..self.items.len() {
            let it = &self.items[i];
            let label = zone_label(&it.z);
            let parts = match &self.fixed[i] {
                Some(p) => Some(p.clone()),
                None => {
                    let key = Self::parent_key(it);
                    if key.is_empty() {
                        None
                    } else {
                        let (live, below) = self.dead_chain(&key);
                        match resolved.get(&live) {
                            Some(base) => {
                                let mut p = base.clone();
                                p.extend(below);
                                Some(p)
                            }
                            // Apple says ID_INVALID for a deleted folder: keep its files together.
                            None if !live.is_empty() => {
                                self.unplaced.push(i);
                                let mut p = vec![GROUPS.to_string(), group_name(&live)];
                                p.extend(below);
                                Some(p)
                            }
                            None => None,
                        }
                    }
                }
            };
            let parts = match parts {
                Some(mut p) => {
                    if let Some(l) = label {
                        p.insert(0, l);
                    }
                    p
                }
                None => {
                    self.unplaced.push(i);
                    vec![UNKNOWN.to_string()]
                }
            };
            let f = self.folder(&parts);
            self.items[i].f = f;
        }
    }

    /// Files and bytes per folder, subfolders included.
    pub fn stats(&self) -> Vec<FolderStat> {
        let mut files = vec![0u64; self.folders.len()];
        let mut bytes = vec![0u64; self.folders.len()];
        for it in &self.items {
            let mut f = it.f as i32;
            while f >= 0 {
                files[f as usize] += 1;
                bytes[f as usize] += it.s;
                f = self.folders[f as usize].parent;
            }
        }
        let groups = self.index.get(GROUPS).map(|&g| g as i32);
        let mut hint: Vec<Vec<String>> = vec![Vec::new(); self.folders.len()];
        for it in &self.items {
            let f = it.f as usize;
            if groups.is_some() && self.folders[f].parent == groups.unwrap() && hint[f].len() < 3 {
                hint[f].push(it.n.clone());
            }
        }
        self.folders
            .iter()
            .enumerate()
            .map(|(i, f)| FolderStat {
                name: f.name.clone(),
                parent: f.parent,
                files: files[i],
                bytes: bytes[i],
                hint: std::mem::take(&mut hint[i]),
            })
            .collect()
    }

    pub fn folder(&mut self, parts: &[String]) -> u32 {
        let mut idx = 0u32;
        let mut path = String::new();
        for p in parts {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(p);
            idx = match self.index.get(&path) {
                Some(&i) => i,
                None => {
                    let i = self.folders.len() as u32;
                    self.folders.push(Folder { name: p.clone(), parent: idx as i32, path: path.clone() });
                    self.index.insert(path.clone(), i);
                    i
                }
            };
        }
        idx
    }

    pub fn push(&mut self, doc: &Value) {
        let id = st(doc, &["item_id", "itemId", "drivewsid"]);
        if id.is_empty() || !self.seen.insert(id.clone()) {
            return;
        }
        let kind = st(doc, &["type"]).to_uppercase();
        if kind == "FOLDER" || kind == "APP_LIBRARY" || kind == "APP_CONTAINER" {
            // Deleted folders are tombstones too: keep name + parent so paths can be rebuilt locally.
            let mut ws = st(doc, &["drivewsid"]);
            if ws.is_empty() {
                let di = st(doc, &["docwsid", "document_id"]);
                let z = st(doc, &["zone"]);
                if !di.is_empty() {
                    ws = format!("FOLDER::{}::{di}", if z.is_empty() { "com.apple.CloudDocs" } else { &z });
                }
            }
            if !ws.is_empty() {
                self.dead_folders.insert(ws, (st(doc, &["name"]), st(doc, &["parentId", "parent_id"])));
            }
            return;
        }
        let mut name = st(doc, &["name", "filename", "basename"]);
        let mut ext = st(doc, &["extension"]);
        if !ext.is_empty() && !name.to_lowercase().ends_with(&format!(".{}", ext.to_lowercase())) {
            name = format!("{name}.{ext}");
        } else if ext.is_empty() {
            if let Some((_, e)) = name.rsplit_once('.') {
                if !e.is_empty() && e.len() <= 12 && !name.starts_with('.') {
                    ext = e.to_string();
                }
            }
        }
        let zone = st(doc, &["zone"]);
        let size = doc
            .get("size")
            .and_then(num)
            .or_else(|| doc.pointer("/data/size").and_then(num))
            .or_else(|| doc.get("file_size").and_then(num))
            .unwrap_or(0.0) as u64;
        let item = Item {
            di: st(doc, &["document_id", "documentId", "docwsid"]),
            p: st(doc, &["parent_id", "parentId"]),
            z: zone.clone(),
            e: ext.to_lowercase(),
            s: size,
            d: time_ms(doc, &["date_deleted", "dateDeleted", "deleted_at", "deletedAt", "deleted_time", "date_trashed", "trashed_at", "tombstone_date", "dtime", "deleted", "dateChanged"]),
            m: time_ms(doc, &["mtime", "date_modified", "dateModified", "modified"]),
            c: time_ms(doc, &["btime", "date_created", "dateCreated", "created"]),
            x: doc.pointer("/file_flags/is_executable").and_then(Value::as_bool).unwrap_or(false),
            t: st(doc, &["etag"]),
            u: false,
            o: String::new(),
            f: 0,
            n: name.clone(),
            i: id,
        };
        self.fixed.push(restore_path(doc, &name));
        self.items.push(item);
    }

    pub fn finish(self, dsid: String, now: i64) -> Snapshot {
        let partial = !self.unplaced.is_empty();
        Snapshot { scanned_at: now, dsid, items: self.items, folders: self.folders, partial_paths: partial, copies_checked: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn iso_and_epoch_times() {
        assert_eq!(parse_iso("1970-01-01T00:00:01Z"), Some(1000));
        assert_eq!(parse_iso("2026-11-02T10:00:00.250+02:00"), Some(1793606400250));
        assert_eq!(time_ms(&json!({"mtime": 1_700_000_000}), &["mtime"]), 1_700_000_000_000);
        assert_eq!(time_ms(&json!({"mtime": 1_700_000_000_000i64}), &["mtime"]), 1_700_000_000_000);
    }

    #[test]
    fn builds_tree_from_restore_path() {
        let mut b = Builder::new();
        b.push(&json!({"item_id":"a","name":"x","extension":"py","restorePath":"/Code/proj/src/","size":10}));
        b.push(&json!({"item_id":"b","name":"y.py","restorePath":["Code","proj"],"size":5}));
        b.push(&json!({"item_id":"c","name":"z.txt"}));
        b.push(&json!({"item_id":"c","name":"z.txt"}));
        b.relayout(&HashMap::new());
        assert_eq!(b.items.len(), 3);
        let snap = b.finish("1".into(), 0);
        assert_eq!(snap.items[0].n, "x.py");
        assert_eq!(snap.items[0].e, "py");
        assert_eq!(snap.folders[snap.items[0].f as usize].path, "Code/proj/src");
        assert_eq!(snap.folders[snap.items[1].f as usize].path, "Code/proj");
        assert!(snap.partial_paths);
        assert_eq!(snap.folders[snap.items[2].f as usize].path, UNKNOWN);
    }

    #[test]
    fn places_through_deleted_folders() {
        let mut b = Builder::new();
        b.push(&json!({"item_id":"f1","name":"a","extension":"txt","parentId":"FOLDER::com.apple.CloudDocs::D1","size":3}));
        b.relayout(&HashMap::new());
        assert_eq!(b.wanted(&HashSet::new()), vec!["FOLDER::com.apple.CloudDocs::D1".to_string()]);
        assert_eq!(b.folders[b.items[0].f as usize].path, "Deleted folders/Deleted folder D1");
        assert_eq!(b.stats()[b.items[0].f as usize].hint, vec!["a.txt".to_string()]);
        // the deleted folder shows up in a later page
        b.push(&json!({"item_id":"d1","type":"FOLDER","name":"Old","drivewsid":"FOLDER::com.apple.CloudDocs::D1","parentId":"FOLDER::com.apple.CloudDocs::L"}));
        let mut r = HashMap::new();
        r.insert("FOLDER::com.apple.CloudDocs::L".to_string(), vec!["Docs".to_string()]);
        b.relayout(&r);
        let st = b.stats();
        assert_eq!(st[0].files, 1);
        assert_eq!(st[0].bytes, 3);
        assert_eq!(b.folders[b.items[0].f as usize].path, "Docs/Old");
        assert!(b.unplaced.is_empty());
    }
}
