//! Likely copies: files with the same name, size and modified time. The list shows each
//! once. Apple cannot read a deleted file, so the bytes are compared only at restore time
//! (see restore.rs): a hidden copy is always restored with the file that is shown.

use std::collections::HashMap;

use crate::model::{Snapshot, GROUPS};

/// Mark copies in `snap`. Returns how many are hidden.
pub fn mark(snap: &mut Snapshot) -> usize {
    let mut groups: HashMap<(String, u64, i64), Vec<usize>> = HashMap::new();
    for (i, it) in snap.items.iter().enumerate() {
        if it.m > 0 {
            groups.entry((it.n.clone(), it.s, it.m)).or_default().push(i);
        }
    }
    for it in snap.items.iter_mut() {
        it.u = false;
        it.o.clear();
    }
    let mut hidden = 0;
    for mut same in groups.into_values().filter(|v| v.len() > 1) {
        // Show the one whose folder is known, then the shortest path.
        same.sort_by_key(|&i| {
            let path = &snap.folders[snap.items[i].f as usize].path;
            (path.starts_with(GROUPS), path.len(), snap.items[i].i.clone())
        });
        let keep = snap.items[same[0]].i.clone();
        for &i in &same[1..] {
            snap.items[i].u = true;
            snap.items[i].o = keep.clone();
            hidden += 1;
        }
    }
    snap.copies_checked = true;
    tracing::info!(target: "copies", hidden, "likely copies hidden");
    hidden
}
