"use strict";
// Salvage UI. Vanilla JS, no build step. Talks to Rust through window.__TAURI__.

const T = window.__TAURI__;
const invoke = (cmd, args) => T.core.invoke(cmd, args);
const listen = (ev, fn) => T.event.listen(ev, (e) => fn(e.payload));
const $ = (id) => document.getElementById(id);
const DAY = 86400000;
const KEEP_DAYS = 30;
const ROW_H = 30;

// ---------------- helpers ----------------
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const nf = new Intl.NumberFormat();
const coll = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" }).compare;
const n = (x) => nf.format(x);
const plural = (x, w) => `${n(x)} ${w}${x === 1 ? "" : "s"}`;
function bytes(b) {
  if (!b) return "—";
  const u = ["bytes", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (b >= 1000 && i < u.length - 1) { b /= 1000; i++; }
  return i === 0 ? `${b} bytes` : `${b < 10 ? b.toFixed(1) : Math.round(b)} ${u[i]}`;
}
const thisYear = new Date().getFullYear();
const dFmt = new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short" });
const dyFmt = new Intl.DateTimeFormat(undefined, { day: "numeric", month: "short", year: "numeric" });
const tFmt = new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" });
const fullFmt = new Intl.DateTimeFormat(undefined, { dateStyle: "full", timeStyle: "short" });
function when(ms) {
  if (!ms) return "—";
  const d = new Date(ms);
  return d.getFullYear() === thisYear ? `${dFmt.format(d)}, ${tFmt.format(d)}` : dyFmt.format(d);
}
function expires(item) {
  if (!item.d) return { text: "—", cls: "" };
  const left = item.d + KEEP_DAYS * DAY - Date.now();
  if (left <= 0) return { text: "any time", cls: "exp-soon" };
  const days = left / DAY;
  const text = days < 1 ? `${Math.max(1, Math.round(left / 3600000))} h` : `${Math.floor(days)} days`;
  return { text, cls: days < 3 ? "exp-soon" : days < 7 ? "exp-mid" : "" };
}
function ago(ms) {
  const s = (Date.now() - ms) / 1000;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return when(ms);
}
function dur(sec) {
  if (!isFinite(sec) || sec < 0) return "—";
  if (sec < 60) return `${Math.max(1, Math.round(sec))} s`;
  if (sec < 3600) return `${Math.floor(sec / 60)} min ${Math.round(sec % 60)} s`;
  return `${Math.floor(sec / 3600)} h ${Math.round((sec % 3600) / 60)} min`;
}

const EXT_COLORS = {
  py: "#3572A5", js: "#d4a500", mjs: "#d4a500", ts: "#2f74c0", tsx: "#2f74c0", jsx: "#d4a500", json: "#e07b00",
  md: "#6b7280", txt: "#6b7280", pdf: "#e5383b", png: "#1f9d55", jpg: "#1f9d55", jpeg: "#1f9d55", heic: "#1f9d55",
  gif: "#1f9d55", svg: "#f59e0b", html: "#e34c26", css: "#7c3aed", scss: "#c6538c", rs: "#b7410e", go: "#00a7c4",
  swift: "#f05138", java: "#b07219", kt: "#a97bff", c: "#555", h: "#555", cpp: "#f34b7d", rb: "#cc342d",
  sh: "#4eaa25", yml: "#cb171e", yaml: "#cb171e", toml: "#9c4221", csv: "#16a34a", xlsx: "#16a34a", docx: "#2b579a",
  pptx: "#d24726", zip: "#8b5cf6", mov: "#0ea5e9", mp4: "#0ea5e9", mp3: "#ec4899", ipynb: "#f37626", sql: "#e38c00",
  pages: "#f59e0b", numbers: "#16a34a", key: "#2563eb",
};
function extColor(e) {
  if (EXT_COLORS[e]) return EXT_COLORS[e];
  let h = 0;
  for (const c of e) h = (h * 31 + c.charCodeAt(0)) % 360;
  return `hsl(${h} 45% 48%)`;
}
const FOLDER_SVG = `<svg class="ficon" viewBox="0 0 20 20"><path d="M2 5.2C2 4.5 2.5 4 3.2 4h4.1c.4 0 .7.1.9.4l1.1 1.2h7.5c.7 0 1.2.5 1.2 1.2v8.4c0 .7-.5 1.2-1.2 1.2H3.2C2.5 16.4 2 15.9 2 15.2z" fill="var(--folder)"/><path d="M2 7.4h16v7.8c0 .7-.5 1.2-1.2 1.2H3.2c-.7 0-1.2-.5-1.2-1.2z" fill="#6aaaf8"/></svg>`;
const CLOUD_SVG = `<svg viewBox="0 0 20 20"><path d="M6 15.5a3.5 3.5 0 0 1-.4-7A4.8 4.8 0 0 1 14.8 7a3.3 3.3 0 0 1 .2 6.6V15.5z" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/></svg>`;
const MAC_SVG = `<svg viewBox="0 0 20 20"><rect x="2.5" y="3.5" width="15" height="10" rx="1.5" fill="none" stroke="currentColor" stroke-width="1.5"/><path d="M7 16.5h6M10 13.5v3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>`;
const BACK_SVG = `<svg viewBox="0 0 20 20"><path d="M8 5 3.5 9.5 8 14M4 9.5h7.5a5 5 0 0 1 0 10" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" transform="translate(0 -2)"/></svg>`;
const DISC_SVG = `<svg viewBox="0 0 10 10"><path d="M3.5 1.5 7 5l-3.5 3.5" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
const extBadge = (e) => `<span class="ext-badge" style="--c:${extColor(e)}"><span>${esc((e || "?").slice(0, 4).toUpperCase())}</span></span>`;

// ---------------- state ----------------
const S = {
  hide: loadHide(),
  account: null,
  scannedAt: 0,
  partial: false,
  items: [],
  folders: [],
  restored: new Set(),
  // derived
  children: [], order: null, pos: null, sub: null, P: null, B: null, R: null,
  done: null, sel: null, selCount: null, selTotal: 0, selBytes: 0,
  // view
  cur: 0, view: "folders", q: "", whenDays: 0, ext: null,
  sort: { key: "name", dir: 1 },
  rows: [], cursor: -1, anchor: -1,
  expanded: new Set([0]),
  inspector: false,
  dest: loadDest(),
  job: null,
};

function loadDest() {
  try { return JSON.parse(localStorage.getItem("salvage.dest")) || { kind: "original" }; } catch { return { kind: "original" }; }
}
function saveDest() {
  try { localStorage.setItem("salvage.dest", JSON.stringify(S.dest)); } catch {}
}

// Build all indexes once per snapshot. Items get sorted by folder in depth-first
// order, so any folder's whole subtree is one contiguous range of S.order.
function load(view) {
  if (!view) return;
  const snap = view.snapshot;
  S.items = snap.items;
  S.lc = null;
  kindsCache = null;
  S.copiesChecked = !!snap.copiesChecked;
  S.folders = snap.folders;
  S.scannedAt = snap.scannedAt;
  S.partial = snap.partialPaths;
  S.restored = new Set(view.restored || []);
  const F = S.folders.length, N = S.items.length;

  S.children = Array.from({ length: F }, () => []);
  for (let f = 1; f < F; f++) S.children[S.folders[f].parent].push(f);
  for (const c of S.children) c.sort((a, b) => coll(S.folders[a].name, S.folders[b].name));

  // dfs position + subtree size per folder
  S.pos = new Int32Array(F);
  S.sub = new Int32Array(F);
  const dfs = [];
  const stack = [0];
  while (stack.length) {
    const f = stack.pop();
    S.pos[f] = dfs.length;
    dfs.push(f);
    const ch = S.children[f];
    for (let i = ch.length - 1; i >= 0; i--) stack.push(ch[i]);
  }
  for (let k = F - 1; k >= 0; k--) {
    const f = dfs[k];
    let size = 1;
    for (const c of S.children[f]) size += S.sub[c];
    S.sub[f] = size;
  }
  // counting sort of items by folder dfs position
  const cnt = new Int32Array(F + 1);
  for (let i = 0; i < N; i++) cnt[S.pos[S.items[i].f] + 1]++;
  for (let k = 0; k < F; k++) cnt[k + 1] += cnt[k];
  S.P = cnt.slice();
  const fill = cnt.slice();
  S.order = new Int32Array(N);
  for (let i = 0; i < N; i++) S.order[fill[S.pos[S.items[i].f]]++] = i;
  S.B = new Float64Array(N + 1);
  S.R = new Int32Array(N + 1);
  S.done = new Uint8Array(N);
  for (let k = 0; k < N; k++) {
    const it = S.items[S.order[k]];
    const d = S.restored.has(it.i) ? 1 : 0;
    S.done[S.order[k]] = d;
    S.B[k + 1] = S.B[k] + it.s;
    S.R[k + 1] = S.R[k] + d;
  }
  countSoon();
  // keep selection of ids that still exist
  const prevSel = S.sel ? new Set(selectedIds()) : new Set();
  S.sel = new Uint8Array(N);
  S.selCount = new Int32Array(F);
  S.selTotal = 0;
  S.selBytes = 0;
  applyHide();
  // Deleted folders have no real names, so show the biggest first (display order only).
  const g = S.folders.findIndex((x) => x.parent === 0 && x.name === GROUPS);
  if (g > 0) S.children[g].sort((a, b) => rawCount(b) - rawCount(a));
  for (let i = 0; i < N; i++) if (prevSel.has(S.items[i].i) && !S.done[i]) setSel(i, true);
  if (S.cur >= F) S.cur = 0;
  renderAll();
}

const rangeOf = (f) => [S.P[S.pos[f]], S.P[S.pos[f] + S.sub[f]]];
const directRange = (f) => [S.P[S.pos[f]], S.P[S.pos[f] + 1]];
// raw = everything Apple has; sub* = only file types that are not hidden
const rawCount = (f) => { const [a, b] = rangeOf(f); return b - a; };
const rawBytes = (f) => { const [a, b] = rangeOf(f); return S.B[b] - S.B[a]; };
const subCount = (f) => { const [a, b] = rangeOf(f); return S.VC[b] - S.VC[a]; };
const subBytes = (f) => { const [a, b] = rangeOf(f); return S.VB[b] - S.VB[a]; };
const subSelectable = (f) => { const [a, b] = rangeOf(f); return S.X[b] - S.X[a]; };

// ---------------- file types ----------------
function loadHide() {
  try { return new Set(JSON.parse(localStorage.getItem("salvage.hide")) || []); } catch { return new Set(); }
}
function saveHide() {
  try { localStorage.setItem("salvage.hide", JSON.stringify([...S.hide])); } catch {}
}
// Hidden types are gone everywhere: rows, counts, sizes and selection.
function applyHide() {
  const N = S.items.length;
  S.hid = new Uint8Array(N);
  S.VC = new Int32Array(N + 1);
  S.VB = new Float64Array(N + 1);
  S.X = new Int32Array(N + 1);
  for (let k = 0; k < N; k++) {
    const i = S.order[k];
    const it = S.items[i];
    const h = S.hide.has(it.e) ? 1 : 0;
    S.hid[i] = h;
    S.VC[k + 1] = S.VC[k] + (h ? 0 : 1);
    S.VB[k + 1] = S.VB[k] + (h ? 0 : it.s);
    S.X[k + 1] = S.X[k] + (h || S.done[i] ? 0 : 1);
  }
  if (S.sel) for (let i = 0; i < N; i++) if (S.sel[i] && S.hid[i]) setSel(i, false);
}
function setHide(next) {
  S.hide = next;
  saveHide();
  applyHide();
  S.cursor = -1;
  renderAll();
}
function ancestors(f, fn) { while (f >= 0) { fn(f); f = S.folders[f].parent; } }
function setSel(i, on) {
  if (S.done[i] || !!S.sel[i] === on || (on && S.hid[i])) return;
  S.sel[i] = on ? 1 : 0;
  const d = on ? 1 : -1;
  S.selTotal += d;
  S.selBytes += d * S.items[i].s;
  ancestors(S.items[i].f, (f) => (S.selCount[f] += d));
}
function folderState(f) {
  const total = subSelectable(f);
  const c = S.selCount[f];
  if (!total) return "off-disabled";
  return c === 0 ? "" : c >= total ? "on" : "mixed";
}
function toggleFolder(f, on) {
  if (on === undefined) on = folderState(f) !== "on";
  const [a, b] = rangeOf(f);
  for (let k = a; k < b; k++) setSel(S.order[k], on);
}
function selectedIds() {
  const out = [];
  if (!S.sel) return out;
  for (let i = 0; i < S.sel.length; i++) if (S.sel[i]) out.push(S.items[i].i);
  return out;
}

// ---------------- rows ----------------
function filtersActive() { return !!(S.q || S.whenDays); }
function matcher() {
  const q = S.q.toLowerCase();
  const since = S.whenDays ? Date.now() - S.whenDays * DAY : 0;
  if (q && !S.lc) S.lc = S.items.map((it) => it.n.toLowerCase());
  const lc = S.lc;
  return (i) => !S.hid[i] && (!q || lc[i].includes(q)) && (!since || S.items[i].d >= since);
}
function itemCmp() {
  const { key, dir } = S.sort;
  const by = {
    name: (a, b) => coll(a.n, b.n),
    // folders are numbered in tree order, so no path strings to compare
    loc: (a, b) => S.pos[a.f] - S.pos[b.f] || coll(a.n, b.n),
    size: (a, b) => a.s - b.s,
    mod: (a, b) => a.m - b.m,
    del: (a, b) => a.d - b.d,
    exp: (a, b) => a.d - b.d,
  }[key];
  return (x, y) => dir * by(S.items[x], S.items[y]);
}
function buildRows() {
  const rows = [];
  const flat = S.view === "flat" || filtersActive();
  if (!flat) {
    const fs = S.children[S.cur].filter((f) => subCount(f) > 0);
    const { key, dir } = S.sort;
    if (key === "size") fs.sort((a, b) => dir * (subBytes(a) - subBytes(b)));
    else if (key === "name" && dir < 0) fs.reverse();
    for (const f of fs) rows.push({ t: "f", i: f });
    const [a, b] = directRange(S.cur);
    const its = Array.from(S.order.subarray(a, b)).filter((i) => !S.hid[i]).sort(itemCmp());
    for (const i of its) rows.push({ t: "i", i });
    S.shown = its.length;
  } else {
    const m = matcher();
    const [a, b] = rangeOf(S.cur);
    const its = [];
    for (let k = a; k < b; k++) { const i = S.order[k]; if (m(i)) its.push(i); }
    its.sort(itemCmp());
    for (const i of its) rows.push({ t: "i", i });
    S.shown = its.length;
  }
  S.rows = rows;
  S.flat = flat;
  if (S.cursor >= rows.length) S.cursor = rows.length - 1;
}

// ---------------- render ----------------
function renderAll(reflatten = true) {
  buildRows();
  renderTree(reflatten);
  renderKinds();
  renderCrumbs();
  renderHead();
  renderRows(true);
  renderSelbar();
  renderInspector();
  renderStatus();
  renderAccount();
  $("banner").hidden = !S.hide.size;
  if (S.hide.size) $("banner").textContent = `Hidden file types: ${[...S.hide].slice(0, 12).map((e) => (e ? "." + e : "no extension")).join(", ")}${S.hide.size > 12 ? ` and ${S.hide.size - 12} more` : ""}. Hidden files are not shown and not restored.`;
}

// The tree can have thousands of rows (one per deleted folder), so only the rows
// you can see are drawn. TREE_ROWS is the flat list of open rows: [folder, depth].
const TREE_H = 26;
const TREE_FOLDER_SVG = FOLDER_SVG.replace('class="ficon"', 'class="ticon"');
let TREE_ROWS = [];
function flattenTree() {
  const out = [];
  if (!S.folders.length) { TREE_ROWS = out; return; }
  const stack = [[0, 0]];
  while (stack.length) {
    const [f, depth] = stack.pop();
    out.push([f, depth]);
    if (!S.expanded.has(f)) continue;
    const ch = S.children[f];
    for (let i = ch.length - 1; i >= 0; i--) if (subCount(ch[i]) > 0) stack.push([ch[i], depth + 1]);
  }
  TREE_ROWS = out;
}
function renderTree(reflatten = true) {
  if (reflatten) flattenTree();
  const sc = $("tree-scroll");
  $("tree-spacer").style.height = `${TREE_ROWS.length * TREE_H}px`;
  const first = Math.max(0, Math.floor(sc.scrollTop / TREE_H) - 10);
  const last = Math.min(TREE_ROWS.length, Math.ceil((sc.scrollTop + sc.clientHeight) / TREE_H) + 10);
  const out = [];
  for (let r = first; r < last; r++) {
    const [f, depth] = TREE_ROWS[r];
    const ch = S.children[f];
    const open = S.expanded.has(f);
    out.push(`<div class="tree-row${f === S.cur ? " active" : ""}" data-f="${f}" style="padding-left:${4 + depth * 14}px">
      <span class="disc${ch.length ? (open ? " open" : "") : " none"}" data-act="disc">${DISC_SVG}</span>
      <span class="check ${folderState(f)}" data-act="check"></span>
      ${f === 0 ? `<span class="ticon" style="color:var(--folder)">${CLOUD_SVG}</span>` : TREE_FOLDER_SVG}
      <span class="tname">${esc(S.folders[f].name)}</span>
      <span class="tcount">${n(subCount(f))}</span>
    </div>`);
  }
  const t = $("tree");
  t.style.top = `${first * TREE_H}px`;
  t.innerHTML = out.join("");
}
// Keep the open folder visible in the tree.
function revealInTree(f) {
  const r = TREE_ROWS.findIndex((x) => x[0] === f);
  if (r < 0) return;
  const sc = $("tree-scroll");
  const y = r * TREE_H;
  if (y < sc.scrollTop || y + TREE_H > sc.scrollTop + sc.clientHeight) sc.scrollTop = Math.max(0, y - sc.clientHeight / 3);
}

// Every file type in the open folder (and below), with a checkbox to show or hide it.
let kindsCache = null;
function kindsHere() {
  if (kindsCache && kindsCache.cur === S.cur) return kindsCache.all;
  const counts = new Map();
  const [a, b] = rangeOf(S.cur);
  for (let k = a; k < b; k++) {
    const it = S.items[S.order[k]];
    const c = counts.get(it.e);
    if (c) { c[0]++; c[1] += it.s; } else counts.set(it.e, [1, it.s]);
  }
  const all = [...counts.entries()].sort((x, y) => y[1][0] - x[1][0]);
  kindsCache = { cur: S.cur, all };
  return all;
}
function renderKinds() {
  const all = kindsHere();
  const hiddenHere = all.filter(([e]) => S.hide.has(e)).length;
  $("kinds-sub").textContent = all.length ? `${all.length - hiddenHere} of ${all.length} shown` : "";
  const kq = ($("kinds-q").value || "").trim().toLowerCase().replace(/^\./, "");
  $("kinds").innerHTML = all
    .filter(([e]) => !kq || (e || "no extension").toLowerCase().includes(kq))
    .map(([e, [c, by]]) => `<div class="tree-row kind-row" data-ext="${esc(e)}" title="${bytes(by)} · ⌥-click: show only this type">
      <span class="check ${S.hide.has(e) ? "" : "on"}"></span>
      <span class="ext-dot" style="background:${extColor(e)}"></span>
      <span class="tname">${e ? "." + esc(e) : "No extension"}</span><span class="tcount">${n(c)}</span></div>`)
    .join("");
}

function renderCrumbs() {
  const chain = [];
  ancestors(S.cur, (f) => chain.unshift(f));
  $("crumbs").innerHTML = chain
    .map((f, k) => `${k ? '<span class="crumb-sep">›</span>' : ""}<button class="crumb${k === chain.length - 1 ? " last" : ""}" data-f="${f}">${esc(S.folders[f].name)}</button>`)
    .join("");
  $("btn-back").disabled = S.cur === 0;
  document.querySelectorAll("#seg-view button").forEach((b) => b.classList.toggle("on", b.dataset.view === (S.flat ? "flat" : "folders")));
}

function renderHead() {
  document.querySelector(".table-wrap").classList.toggle("flat-off", !S.flat);
  document.querySelectorAll(".thead .sortable").forEach((h) => {
    h.classList.remove("asc", "desc");
    if (h.dataset.sort === S.sort.key) h.classList.add(S.sort.dir > 0 ? "asc" : "desc");
  });
  // header checkbox reflects rows shown
  let sel = 0, tot = 0;
  for (const r of S.rows) {
    if (r.t === "f") { tot += subSelectable(r.i); sel += S.selCount[r.i]; }
    else if (!S.done[r.i]) { tot++; sel += S.sel[r.i]; }
  }
  $("check-all").className = `check ${!tot ? "off-disabled" : sel === 0 ? "" : sel >= tot ? "on" : "mixed"}`;
}

let lastRange = "";
function renderRows(force) {
  const sc = $("scroller");
  $("spacer").style.height = `${S.rows.length * ROW_H}px`;
  const first = Math.max(0, Math.floor(sc.scrollTop / ROW_H) - 8);
  const last = Math.min(S.rows.length, Math.ceil((sc.scrollTop + sc.clientHeight) / ROW_H) + 8);
  const key = `${first}:${last}`;
  if (!force && key === lastRange) return;
  lastRange = key;
  const html = [];
  for (let r = first; r < last; r++) html.push(rowHtml(r));
  $("rows").innerHTML = html.join("");
  const empty = $("empty");
  empty.hidden = S.rows.length > 0;
  if (!S.rows.length) {
    empty.innerHTML = filtersActive()
      ? `<div><svg viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.6" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="m10.5 10.5 3.5 3.5" stroke="currentColor" stroke-width="1.2"/></svg><h3>No matches</h3>Try another word or date.</div>`
      : `<div><svg viewBox="0 0 16 16"><path d="M1.5 4.5a1 1 0 0 1 1-1h3.2l1.3 1.4h6.5a1 1 0 0 1 1 1v6.6a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" fill="none" stroke="currentColor" stroke-width="1"/></svg><h3>Nothing deleted here</h3></div>`;
  }
}

function rowHtml(r) {
  const row = S.rows[r];
  const cur = r === S.cursor ? " cursor" : "";
  const top = `top:${r * ROW_H}px`;
  if (row.t === "f") {
    const f = row.i;
    const st = folderState(f);
    const c = subCount(f);
    return `<div class="row folder${st === "on" ? " checked" : ""}${cur}" data-r="${r}" style="${top}">
      <div class="c-check"><span class="check ${st}" data-act="check"></span></div>
      <div class="c-name">${FOLDER_SVG}<span class="nm">${esc(S.folders[f].name)}</span></div>
      <div class="c-loc"></div>
      <div class="c-size">${bytes(subBytes(f))}</div>
      <div class="c-mod">${plural(c, "file")}</div>
      <div class="c-del"></div><div class="c-exp"></div>
    </div>`;
  }
  const it = S.items[row.i];
  const done = S.done[row.i];
  const ex = expires(it);
  return `<div class="row${S.sel[row.i] ? " checked" : ""}${done ? " done" : ""}${cur}" data-r="${r}" style="${top}">
    <div class="c-check"><span class="check ${done ? "off-disabled" : S.sel[row.i] ? "on" : ""}" data-act="check"></span></div>
    <div class="c-name">${extBadge(it.e)}<span class="nm">${esc(it.n)}</span>${done ? '<span class="pill">Restored</span>' : ""}</div>
    <div class="c-loc">${esc(S.folders[it.f].path || "iCloud Drive")}</div>
    <div class="c-size">${bytes(it.s)}</div>
    <div class="c-mod">${when(it.m)}</div>
    <div class="c-del">${when(it.d)}</div>
    <div class="c-exp ${done ? "" : ex.cls}">${done ? "" : ex.text}</div>
  </div>`;
}

function renderSelbar() {
  const bar = $("selbar");
  bar.hidden = S.selTotal === 0;
  if (!S.selTotal) return;
  $("sel-count").innerHTML = `${plural(S.selTotal, "file")}<small>${bytes(S.selBytes)}</small>`;
  const d = S.dest;
  $("dest-icon").innerHTML = d.kind === "local" ? MAC_SVG : d.kind === "icloud" ? CLOUD_SVG : BACK_SVG;
  $("dest-text").textContent = destLabel(d);
}
function destLabel(d) {
  if (d.kind === "icloud") return `iCloud Drive › ${d.path || "/"}`;
  if (d.kind === "local") return d.path.split("/").filter(Boolean).slice(-2).join(" › ");
  return "Where they were";
}

function countSoon() {
  const limit = Date.now() - (KEEP_DAYS - 3) * DAY;
  let soon = 0;
  for (let i = 0; i < S.items.length; i++) { const d = S.items[i].d; if (d && d < limit && !S.done[i]) soon++; }
  S.soon = soon;
}
function renderStatus() {
  const total = S.items.length;
  const left = total - S.R[total];
  $("status-text").textContent =
    `${plural(S.shown, "file")} shown · ${n(left)} of ${n(total)} still deleted` + (S.soon ? ` · ${n(S.soon)} gone in under 3 days` : "");
}

function renderAccount() {
  const a = S.account;
  if (!a) return;
  $("acct-name").textContent = a.name;
  $("acct-sub").textContent = S.scannedAt ? `Scanned ${ago(S.scannedAt)}` : a.appleId;
  $("acct-avatar").textContent = a.name.split(/\s+/).map((w) => w[0] || "").join("").slice(0, 2).toUpperCase();
}

function renderInspector() {
  const el = $("inspector");
  el.hidden = !S.inspector;
  if (!S.inspector) return;
  const row = S.rows[S.cursor];
  if (!row) { el.innerHTML = `<div class="insp-empty">Select a file to see its details.</div>`; return; }
  if (row.t === "f") {
    const f = row.i;
    const [a, b] = rangeOf(f);
    let newest = 0, oldest = Infinity;
    for (let k = a; k < b; k++) { const d = S.items[S.order[k]].d; if (d) { newest = Math.max(newest, d); oldest = Math.min(oldest, d); } }
    el.innerHTML = `<div class="insp-head"><div class="big">${FOLDER_SVG}</div>
      <div class="insp-name">${esc(S.folders[f].name)}</div><div class="insp-sub">Folder</div></div>
      <dl class="kv"><dt>Deleted files</dt><dd>${n(b - a)}</dd><dt>Size</dt><dd>${bytes(S.B[b] - S.B[a])}</dd>
      <dt>Selected</dt><dd>${n(S.selCount[f])}</dd><dt>Where</dt><dd>${esc(S.folders[f].path || "iCloud Drive")}</dd>
      ${newest ? `<dt>Deleted</dt><dd>${when(oldest)} – ${when(newest)}</dd>` : ""}</dl>`;
    return;
  }
  const it = S.items[row.i];
  const ex = expires(it);
  el.innerHTML = `<div class="insp-head"><div class="big">${extBadge(it.e)}</div>
    <div class="insp-name">${esc(it.n)}</div><div class="insp-sub">${bytes(it.s)}${S.done[row.i] ? " · Restored" : ""}</div></div>
    <dl class="kv">
      <dt>Was in</dt><dd>${esc("iCloud Drive/" + (S.folders[it.f].path || ""))}</dd>
      <dt>Created</dt><dd>${it.c ? fullFmt.format(it.c) : "—"}</dd>
      <dt>Modified</dt><dd>${it.m ? fullFmt.format(it.m) : "—"}</dd>
      <dt>Deleted</dt><dd>${it.d ? fullFmt.format(it.d) : "—"}</dd>
      <dt>Gone in</dt><dd class="${ex.cls}">${S.done[row.i] ? "—" : ex.text}</dd>
      ${it.x ? "<dt>Executable</dt><dd>Yes</dd>" : ""}
      <dt>Item ID</dt><dd class="mono">${esc(it.i)}</dd>
    </dl>`;
}

const refreshSelection = () => { renderTree(); renderHead(); renderRows(true); renderSelbar(); renderInspector(); };

// ---------------- navigation ----------------
function go(f) {
  if (f == null || f < 0 || f >= S.folders.length) return;
  S.cur = f;
  ancestors(f, (a) => S.expanded.add(a));
  S.cursor = -1;
  $("scroller").scrollTop = 0;
  flattenTree();
  revealInTree(f);
  renderAll(false);
}
function setCursor(r, scroll = true) {
  if (!S.rows.length) return;
  const prev = S.cursor;
  S.cursor = Math.max(0, Math.min(S.rows.length - 1, r));
  if (scroll) {
    const sc = $("scroller");
    const top = S.cursor * ROW_H;
    if (top < sc.scrollTop) sc.scrollTop = top;
    else if (top + ROW_H > sc.scrollTop + sc.clientHeight - 90) sc.scrollTop = top + ROW_H - sc.clientHeight + 90;
  }
  const drawn = lastRange;
  renderRows(false);
  if (lastRange === drawn) {
    $("rows").querySelector(`[data-r="${prev}"]`)?.classList.remove("cursor");
    $("rows").querySelector(`[data-r="${S.cursor}"]`)?.classList.add("cursor");
  }
  renderInspector();
}
function toggleRow(r, on) {
  const row = S.rows[r];
  if (!row) return;
  if (row.t === "f") toggleFolder(row.i, on);
  else if (!S.done[row.i]) setSel(row.i, on === undefined ? !S.sel[row.i] : on);
}
function rowIsOn(r) {
  const row = S.rows[r];
  return row.t === "f" ? folderState(row.i) === "on" : !!S.sel[row.i];
}
function selectAllShown(on) {
  for (let r = 0; r < S.rows.length; r++) toggleRow(r, on);
  refreshSelection();
}

// ---------------- gate ----------------
function showGate(mode, text) {
  $("main").hidden = true;
  $("overview").hidden = true;
  $("gate").hidden = false;
  const busy = mode === "busy";
  $("gate-progress").hidden = !busy;
  $("btn-signin").hidden = busy;
  $("gate-points").hidden = busy;
  $("gate-error").hidden = mode !== "error";
  if (mode === "error") $("gate-error").textContent = text;
  if (busy) $("gate-progress-text").textContent = text;
  $("gate-title").textContent = busy ? "One moment" : "Get your deleted iCloud Drive files back";
  $("gate-sub").hidden = busy;
}
function showMain() {
  $("gate").hidden = true;
  $("overview").hidden = true;
  $("main").hidden = false;
  $("scroller").focus();
}

// ---------------- overview ----------------
// O.folders: [{name, parent, files, bytes}], index 0 = iCloud Drive. Comes from the scan
// events while Apple sends the list, and from the snapshot after.
const O = { folders: [], files: 0, bytes: 0, complete: false, scanning: false, progress: "", open: new Set(), more: new Map(), map: null };
const UNKNOWN = "Folder unknown";
const GROUPS = "Deleted folders";
const gb = (b) => `${(b / 1e9).toFixed(2)} GB`;

function showOverview() {
  $("gate").hidden = true;
  $("main").hidden = true;
  $("overview").hidden = false;
  renderOverview();
}
function overviewFromSnapshot() {
  const g = S.folders.findIndex((f) => f.parent === 0 && f.name === GROUPS);
  O.folders = S.folders.map((f, i) => {
    const o = { name: f.name, parent: f.parent, files: rawCount(i), bytes: rawBytes(i) };
    if (g > 0 && f.parent === g) {
      const [a, b] = directRange(i);
      o.hint = [];
      for (let k = a; k < b && o.hint.length < 3; k++) o.hint.push(S.items[S.order[k]].n);
    }
    return o;
  });
  O.files = S.items.length;
  O.bytes = O.folders.length ? O.folders[0].bytes : 0;
  O.complete = true;
  O.scanning = false;
  O.map = null;
}
// Keys by path, so open folders stay open when the scan lays the tree out again.
function ovKey(i) {
  const parts = [];
  for (let f = i; f > 0; f = O.folders[f].parent) parts.push(O.folders[f].name);
  return parts.reverse().join("/");
}
// Progress events change only the status line, so they redraw only that.
function renderOvState() {
  if ($("overview").hidden) return;
  $("ov-files").textContent = plural(O.files, "file");
  $("ov-bytes").textContent = gb(O.bytes);
  const st = $("ov-state");
  if (O.complete && O.checking) st.innerHTML = `<span class="pill ok">✓ All files found</span><span class="pill wait"><span class="mini"></span>Removing copies</span><span>${esc(O.progress)}</span>`;
  else if (O.complete) st.innerHTML = `<span class="pill ok">✓ All files found</span>`;
  else if (O.scanning) st.innerHTML = `<span class="pill wait"><span class="mini"></span>Finding files</span><span>${esc(O.progress)}</span>`;
  else st.innerHTML = `<span class="pill wait">Not complete</span><span>The scan stopped. Use Scan again.</span>`;
  $("btn-browse").disabled = !O.complete || !!O.checking || !S.items.length;
}
// Many folder events can come close together: draw the table at most 20 times a second.
// A timer, not a frame callback, so a window behind other windows still updates.
let ovTimer = 0;
function scheduleOverview() {
  if (!ovTimer) ovTimer = setTimeout(() => { ovTimer = 0; renderOverview(); }, 50);
}
function renderOverview() {
  if ($("overview").hidden) return;
  renderOvState();

  const F = O.folders.length;
  const kids = Array.from({ length: F }, () => []);
  for (let i = 1; i < F; i++) kids[O.folders[i].parent].push(i);
  const last = (x) => (x === UNKNOWN ? 2 : x === GROUPS ? 1 : 0);
  const cmp = (a, b) => {
    const A = O.folders[a].name, B = O.folders[b].name;
    if (last(A) !== last(B)) return last(A) - last(B);
    // groups of a deleted folder: biggest first
    if (O.folders[a].hint) return O.folders[b].files - O.folders[a].files;
    return coll(A, B);
  };
  for (let i = 0; i < F; i++) kids[i] = kids[i].filter((c) => O.folders[c].files > 0);
  kids.forEach((k) => k.sort(cmp));
  // map path -> snapshot folder index, to open the browser at the right place
  if (O.complete && !O.loading && !O.map) { O.map = new Map(); S.folders.forEach((f, i) => O.map.set(f.path, i)); }

  const status = O.complete ? `<span class="pill ok">✓ All found</span>` : `<span class="pill wait"><span class="mini"></span>Finding files</span>`;
  const folderSvg = `<svg class="fold" viewBox="0 0 16 16"><path d="M1.5 4.5a1 1 0 0 1 1-1h3.2l1.3 1.4h6.5a1 1 0 0 1 1 1v6.6a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1z" fill="currentColor" opacity=".9"/></svg>`;
  const out = [];
  const row = (i, depth, label) => {
    const f = O.folders[i];
    const key = ovKey(i);
    const has = i > 0 && kids[i].length > 0;
    const on = has && O.open.has(key);
    const canOpen = O.complete;
    out.push(`<div class="ov-row" data-key="${esc(key)}" data-i="${i}">
      <div class="ov-name" style="padding-left:${depth * 18}px">
        <button class="ov-caret ${has ? (on ? "on" : "") : "none"}" data-act="toggle" tabindex="-1">▸</button>${folderSvg}
        <span class="label ${canOpen ? "open" : ""}" data-act="open" title="${canOpen ? "Open this folder" : "You can open folders when all files are found"}">${esc(label || f.name)}</span>${f.hint && f.hint.length ? `<span class="hint">e.g. ${esc(f.hint.join(", "))}</span>` : ""}${f.name === GROUPS && i > 0 && O.folders[i].parent === 0 ? `<span class="hint">Apple does not keep the names of deleted folders</span>` : ""}
      </div>
      <div class="ov-num">${n(f.files)}</div>
      <div class="ov-num">${gb(f.bytes)}</div>
      <div class="ov-st">${status}</div>
    </div>`);
    if (on) {
      const limit = O.more.get(key) || 200;
      for (const c of kids[i].slice(0, limit)) row(c, depth + 1);
      const rest = kids[i].length - limit;
      if (rest > 0) out.push(`<div class="ov-row" data-key="${esc(key)}"><div class="ov-name" style="padding-left:${(depth + 1) * 18}px"><button class="ov-caret none" tabindex="-1">▸</button><span class="label open" data-act="more">Show ${n(Math.min(rest, 200))} more (${n(rest)} not shown)</span></div><div class="ov-num"></div><div class="ov-num"></div><div class="ov-st"></div></div>`);
    }
  };
  // files directly in iCloud Drive (not in any folder)
  if (F) {
    const direct = O.folders[0].files - kids[0].reduce((a, c) => a + O.folders[c].files, 0);
    if (direct > 0) {
      const bytesDirect = O.folders[0].bytes - kids[0].reduce((a, c) => a + O.folders[c].bytes, 0);
      out.push(`<div class="ov-row" data-key="" data-i="0"><div class="ov-name"><button class="ov-caret none" tabindex="-1">▸</button>${folderSvg}<span class="label ${O.complete ? "open" : ""}" data-act="open">Top of iCloud Drive</span></div><div class="ov-num">${n(direct)}</div><div class="ov-num">${gb(bytesDirect)}</div><div class="ov-st">${status}</div></div>`);
    }
    for (const c of kids[0]) row(c, 0);
  }
  $("ov-rows").innerHTML = out.join("");
  const empty = $("ov-empty");
  empty.hidden = out.length > 0;
  if (!out.length) empty.textContent = O.scanning ? "Asking Apple for your deleted files…" : "No deleted files found.";
}
function onOverviewClick(e) {
  const el = e.target.closest("[data-act]");
  const r = e.target.closest(".ov-row");
  if (!el || !r) return;
  const key = r.dataset.key;
  if (el.dataset.act === "more") {
    O.more.set(key, (O.more.get(key) || 200) + 200);
    renderOverview();
  } else if (el.dataset.act === "toggle") {
    O.open.has(key) ? O.open.delete(key) : O.open.add(key);
    renderOverview();
  } else if (el.dataset.act === "open") {
    if (O.loading) { toast("Loading the file list. Try again in a moment."); return; }
    if (!O.complete || O.checking) { toast("Wait until the check is done. Then you can open folders."); return; }
    const f = O.map && O.map.get(key);
    showMain();
    go(f == null ? 0 : f);
  }
}

async function signedIn(account, { rescan = false } = {}) {
  S.account = account;
  if (!rescan) {
    const v = await invoke("get_snapshot");
    if (v) { load(v); overviewFromSnapshot(); showOverview(); shown(); checkCopies(); return; }
  }
  await scan();
}

// Once per scan: find exact copies so each file shows once. Runs in the background.
let copiesRunning = false;
async function checkCopies() {
  if (S.copiesChecked || copiesRunning) return;
  copiesRunning = true;
  O.checking = true;
  O.progress = "Checking for exact copies…";
  renderOverview();
  try {
    const v = await invoke("find_copies");
    load(v);
    overviewFromSnapshot();
  } catch (e) {
    handleError(e, "Copy check failed");
  } finally {
    copiesRunning = false;
    O.checking = false;
    renderOverview();
  }
}

async function scan(fresh = false) {
  O.scanning = true;
  O.complete = false;
  O.progress = "Asking Apple for the list…";
  if (fresh) { O.folders = []; O.files = 0; O.bytes = 0; }
  showOverview();
  try {
    const v = await invoke("scan", { fresh });
    load(v);
    overviewFromSnapshot();
    renderOverview();
    await checkCopies();
  } catch (e) {
    O.scanning = false;
    renderOverview();
    handleError(e, "Scan failed");
  }
}

function handleError(e, what) {
  const msg = String(e);
  if (/expired|not signed in/i.test(msg)) {
    S.account = null;
    showGate("error", `${msg}`);
    $("btn-signin").hidden = false;
    $("gate-points").hidden = false;
    return;
  }
  if ($("main").hidden && $("overview").hidden) showGate("error", `${what}: ${msg}`);
  else toast(`${what}: ${msg}`, 6000);
}

let toastTimer;
function toast(text, ms = 2600) {
  const t = $("toast");
  t.textContent = text;
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), ms);
}

// ---------------- menus & modals ----------------
function openMenu(anchor, items, { above = false } = {}) {
  const m = $("menu");
  m.innerHTML = items
    .map((it, k) => (it === "-" ? "<hr>" : `<button data-k="${k}">${it.icon || ""}<span>${esc(it.label)}${it.sub ? `<small>${esc(it.sub)}</small>` : ""}</span>${it.checked ? '<span class="check-mark">✓</span>' : ""}</button>`))
    .join("");
  m.hidden = false;
  const r = anchor.getBoundingClientRect();
  const h = m.offsetHeight, w = m.offsetWidth;
  m.style.left = `${Math.min(window.innerWidth - w - 8, Math.max(8, r.left))}px`;
  m.style.top = above ? `${r.top - h - 6}px` : `${r.bottom + 6}px`;
  m.onclick = (e) => {
    const b = e.target.closest("button[data-k]");
    if (!b) return;
    m.hidden = true;
    items[+b.dataset.k].run();
  };
}
document.addEventListener("mousedown", (e) => { if (!e.target.closest("#menu")) $("menu").hidden = true; });

function modal(html) {
  const root = $("modal-root");
  root.innerHTML = `<div class="scrim"><div class="sheet">${html}</div></div>`;
  return root.querySelector(".sheet");
}
const closeModal = () => ($("modal-root").innerHTML = "");
// A small yes/no sheet. Enter says yes, Escape says no.
function ask(title, text, yes, run, danger = false) {
  const sheet = modal(`<div class="sheet-head"><h2>${esc(title)}</h2><p>${esc(text)}</p></div>
    <div class="sheet-foot"><span class="grow"></span><button class="btn" id="ask-no">Cancel</button><button class="btn primary${danger ? " danger" : ""}" id="ask-yes">${esc(yes)}</button></div>`);
  sheet.querySelector("#ask-no").onclick = closeModal;
  sheet.querySelector("#ask-yes").onclick = () => { closeModal(); run(); };
  sheet.querySelector("#ask-yes").focus();
}
const KEYS = [
  ["↑ ↓", "Move up and down"], ["Space", "Select or clear the file"], ["Shift-click", "Select a range"],
  ["⏎ or →", "Open the folder"], ["⌫ or ←", "Go up one folder"], ["⌘A", "Select all shown"], ["⇧⌘A", "Clear all shown"],
  ["⌘F", "Search"], ["⌘I", "Show file info"], ["⌘R", "Restore the selected files"], ["Esc", "Clear search, then selection"], ["?", "This list"],
];
function showKeys() {
  const sheet = modal(`<div class="sheet-head"><h2>Keyboard shortcuts</h2></div>
    <div class="sheet-body"><dl class="keys">${KEYS.map(([k, v]) => `<dt><kbd>${k}</kbd></dt><dd>${v}</dd>`).join("")}</dl></div>
    <div class="sheet-foot"><span class="grow"></span><button class="btn primary" id="keys-ok">Done</button></div>`);
  sheet.querySelector("#keys-ok").onclick = closeModal;
  sheet.querySelector("#keys-ok").focus();
}
const modalOpen = () => !!$("modal-root").firstChild;

function chooseDestination() {
  const d = S.dest;
  openMenu($("btn-dest"), [
    { icon: BACK_SVG, label: "Where they were", sub: "Each file goes back to its old folder", checked: d.kind === "original", run: () => setDest({ kind: "original" }) },
    { icon: CLOUD_SVG, label: "A folder in iCloud Drive…", sub: "Keeps the folder structure", checked: d.kind === "icloud", run: pickIcloud },
    { icon: MAC_SVG, label: "A folder on this Mac…", sub: "Downloads with dates kept", checked: d.kind === "local", run: pickLocal },
  ], { above: true });
}
function setDest(d) { S.dest = d; saveDest(); renderSelbar(); }

async function pickLocal() {
  const p = await invoke("pick_local_folder");
  if (p) setDest({ kind: "local", path: p });
}

async function pickIcloud() {
  const trail = [{ id: null, name: "iCloud Drive" }];
  const sheet = modal(`<div class="sheet-head"><h2>Choose an iCloud Drive folder</h2><p>Restored files go here, in their folders.</p></div>
    <div class="sheet-body"><div class="picker-crumbs" id="pk-crumbs"></div><div class="picker-list" id="pk-list"></div>
    <div class="newfolder"><input id="pk-new" placeholder="New folder name" spellcheck="false"><button class="btn" id="pk-make">New Folder</button></div></div>
    <div class="sheet-foot"><button class="btn" id="pk-cancel">Cancel</button><button class="btn primary" id="pk-ok">Choose This Folder</button></div>`);
  const cur = () => trail[trail.length - 1];
  const pathOf = () => trail.slice(1).map((t) => t.name).join("/");
  async function show() {
    sheet.querySelector("#pk-crumbs").innerHTML = trail
      .map((t, k) => `${k ? '<span class="crumb-sep">›</span>' : ""}<button class="crumb${k === trail.length - 1 ? " last" : ""}" data-k="${k}">${esc(t.name)}</button>`)
      .join("");
    const list = sheet.querySelector("#pk-list");
    list.innerHTML = `<div class="picker-msg"><div class="spinner" style="margin:0 auto"></div></div>`;
    try {
      const fs = await invoke("list_icloud_folder", { id: cur().id });
      list.innerHTML = fs.length
        ? fs.map((f) => `<div class="picker-row" data-id="${esc(f.id)}" data-name="${esc(f.name)}">${FOLDER_SVG}<span class="tname">${esc(f.name)}</span><span class="go">${DISC_SVG}</span></div>`).join("")
        : `<div class="picker-msg">No folders inside.</div>`;
    } catch (e) {
      list.innerHTML = `<div class="picker-msg">${esc(String(e))}</div>`;
    }
  }
  sheet.querySelector("#pk-crumbs").onclick = (e) => {
    const b = e.target.closest("[data-k]");
    if (b) { trail.length = +b.dataset.k + 1; show(); }
  };
  sheet.querySelector("#pk-list").onclick = (e) => {
    const r = e.target.closest(".picker-row");
    if (r) { trail.push({ id: r.dataset.id, name: r.dataset.name }); show(); }
  };
  sheet.querySelector("#pk-make").onclick = async () => {
    const name = sheet.querySelector("#pk-new").value.trim();
    if (!name) return;
    try {
      const id = await invoke("create_icloud_folder", { parent: cur().id || "FOLDER::com.apple.CloudDocs::root", name });
      trail.push({ id, name });
      sheet.querySelector("#pk-new").value = "";
      show();
    } catch (e) { toast(String(e), 5000); }
  };
  sheet.querySelector("#pk-cancel").onclick = closeModal;
  sheet.querySelector("#pk-ok").onclick = () => {
    setDest({ kind: "icloud", drivewsid: cur().id || "FOLDER::com.apple.CloudDocs::root", path: pathOf() });
    closeModal();
  };
  show();
}

// ---------------- restore ----------------
function confirmRestore() {
  if (!S.selTotal || S.job) return;
  const d = S.dest;
  const where = d.kind === "original" ? "back to where each file was" : d.kind === "icloud" ? `to iCloud Drive › ${d.path || "/"}` : `to ${d.path}`;
  const note = d.kind === "local"
    ? "Apple must first put the files back in iCloud Drive. Then Salvage downloads them here and keeps their dates."
    : d.kind === "icloud"
      ? "Apple first puts the files back where they were. Then Salvage moves them, with their folders."
      : "Files that had the same name as a file there now get a new name from Apple.";
  const sheet = modal(`<div class="sheet-head"><h2>Restore ${plural(S.selTotal, "file")}?</h2><p>${bytes(S.selBytes)}, ${esc(where)}.</p></div>
    <div class="sheet-body"><div class="note">${esc(note)}</div></div>
    <div class="sheet-foot"><button class="btn" id="rc-cancel">Cancel</button><button class="btn primary" id="rc-go">Restore</button></div>`);
  sheet.querySelector("#rc-cancel").onclick = closeModal;
  sheet.querySelector("#rc-go").onclick = startRestore;
  sheet.querySelector("#rc-go").focus();
}

async function startRestore() {
  const ids = selectedIds();
  const dest = S.dest.kind === "icloud" ? { kind: "icloud", drivewsid: S.dest.drivewsid, path: S.dest.path || "" } : S.dest;
  S.job = { started: performance.now(), samples: [], dest, last: null };
  progressSheet();
  try {
    S.job.id = await invoke("start_restore", { ids, destination: dest });
  } catch (e) {
    S.job = null;
    closeModal();
    handleError(e, "Could not start");
  }
}

function progressSheet() {
  const C = 2 * Math.PI * 56;
  modal(`<div class="sheet-head"><h2 id="rp-title">Restoring…</h2><p id="rp-sub"></p></div>
    <div class="sheet-body">
      <div class="rs">
        <div class="ring" id="rp-ring">
          <svg viewBox="0 0 132 132"><defs><linearGradient id="ringGrad" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#5aa2ff"/><stop offset="1" stop-color="#0a5cf0"/></linearGradient></defs>
            <circle class="track" cx="66" cy="66" r="56" fill="none" stroke-width="10"/>
            <circle class="bar" id="rp-bar" cx="66" cy="66" r="56" fill="none" stroke-width="10" stroke-linecap="round" stroke-dasharray="${C}" stroke-dashoffset="${C}"/></svg>
          <div class="pct" id="rp-pct">0%</div>
        </div>
        <div>
          <div class="rs-phase" id="rp-phase">Starting…</div>
          <dl class="rs-stats" id="rp-stats"></dl>
        </div>
      </div>
      <div id="rp-fails"></div>
    </div>
    <div class="sheet-foot"><span class="grow"></span><button class="btn" id="rp-cancel">Stop</button><button class="btn" id="rp-reveal" hidden>Show in Finder</button><button class="btn primary" id="rp-done" hidden>Done</button></div>`);
  $("rp-cancel").onclick = () => { invoke("cancel_restore"); $("rp-cancel").disabled = true; $("rp-cancel").textContent = "Stopping…"; };
  $("rp-done").onclick = finishRestore;
}

function onProgress(p) {
  const job = S.job;
  if (!job) return;
  job.last = p;
  // An empty list means "no new failures", so keep the last full list.
  if (p.failures.length) job.failures = p.failures;
  const now = performance.now();
  const units = p.phase === "recover" ? p.recovered + p.failed : p.placed + p.failed;
  job.samples.push({ t: now, u: units, b: p.bytes, ph: p.phase });
  while (job.samples.length > 2 && now - job.samples[0].t > 4000) job.samples.shift();
  if (!job.timer) job.timer = setTimeout(drawProgress, 50);
}

function drawProgress() {
  const job = S.job;
  if (!job) return;
  job.timer = 0;
  if (!$("rp-bar")) return;
  const p = job.last;
  const now = performance.now();
  const units = p.phase === "recover" ? p.recovered + p.failed : p.placed + p.failed;
  const s0 = job.samples.find((s) => s.ph === p.phase) || job.samples[0];
  const dt = (now - s0.t) / 1000;
  const rate = dt > 0.3 ? (units - s0.u) / dt : 0;
  const brate = dt > 0.3 ? (p.bytes - s0.b) / dt : 0;

  const original = job.dest.kind === "original";
  let frac;
  if (original) frac = (p.recovered + p.failed) / p.total;
  else if (p.phase === "recover") frac = 0.3 * ((p.recovered + p.failed) / p.total);
  else frac = 0.3 + 0.7 * ((p.placed + p.failed) / p.total);
  if (p.done && !p.cancelled && !p.error) frac = 1;
  frac = Math.max(0, Math.min(1, frac || 0));
  const C = 2 * Math.PI * 56;
  $("rp-bar").style.strokeDashoffset = `${C * (1 - frac)}`;
  $("rp-pct").textContent = `${Math.floor(frac * 100)}%`;

  const phaseText = { recover: "Asking Apple to bring files back", move: "Moving files into the folder", download: "Downloading to this Mac" }[p.phase] || "Working";
  $("rp-phase").textContent = p.done ? (p.error ? "Stopped with an error" : p.cancelled ? "Stopped" : "Done") : phaseText;
  const left = p.phase === "recover" ? p.total - p.recovered - p.failed : p.total - p.placed - p.failed;
  const stats = [
    ["Brought back", `${n(p.recovered)} of ${n(p.total)}`],
    ...(original ? [] : [[job.dest.kind === "local" ? "Downloaded" : "Moved", `${n(p.placed)} of ${n(p.recovered)}`]]),
    ...(p.failed ? [["Failed", `<span style="color:var(--danger)">${n(p.failed)}</span>`]] : []),
    ...(job.dest.kind === "local" ? [["Data", `${bytes(p.bytes)} of ${bytes(p.totalBytes)}${brate > 0 && !p.done ? ` · ${bytes(brate)}/s` : ""}`]] : []),
    ...(!p.done && rate > 0 ? [["Speed", `${n(Math.round(rate))} files/s`], ["Time left", dur(left / rate)]] : []),
    ["Time", dur((now - job.started) / 1000)],
  ];
  $("rp-stats").innerHTML = stats.map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`).join("");
  $("rp-sub").textContent = destLabel(job.dest);

  if (job.failures && job.failures !== job.failsDrawn) {
    job.failsDrawn = job.failures;
    const more = job.failures.length > 200 ? `<div><b>…</b><span>and ${n(job.failures.length - 200)} more</span></div>` : "";
    $("rp-fails").innerHTML = `<div class="fails">${job.failures.slice(0, 200).map((f) => `<div><b>${esc(f.name)}</b><span title="${esc(f.error)}">${esc(f.error)}</span></div>`).join("")}${more}</div>`;
  }
  if (p.error) $("rp-fails").insertAdjacentHTML("afterbegin", `<div class="note" style="color:var(--danger)">${esc(p.error)}</div>`);

  if (p.done) {
    $("rp-title").textContent = p.error ? "Restore stopped" : p.cancelled ? "Restore stopped" : `Restored ${plural(original ? p.recovered : p.placed, "file")}`;
    $("rp-cancel").hidden = true;
    $("rp-done").hidden = false;
    $("rp-done").focus();
    if (!p.error && !p.cancelled && !p.failed) {
      document.getElementById("rp-ring").classList.add("ok");
      document.getElementById("rp-pct").outerHTML = `<div class="done-mark"><svg viewBox="0 0 52 52"><path d="M14 27l8 8 17-18" fill="none" stroke="var(--ok)" stroke-width="5" stroke-linecap="round" stroke-linejoin="round"/></svg></div>`;
    }
    if (p.output) {
      $("rp-reveal").hidden = false;
      $("rp-reveal").onclick = () => invoke("open_folder", { path: p.output });
    }
    S.job.finished = true;
  }
}

async function finishRestore() {
  closeModal();
  const p = S.job && S.job.last;
  S.job = null;
  const v = await invoke("get_snapshot");
  load(v);
  if (p && p.error && /expired/i.test(p.error)) handleError(p.error, "Restore");
}

// ---------------- events ----------------
function wire() {
  $("btn-signin").onclick = async () => {
    $("gate-error").hidden = true;
    try { await invoke("sign_in"); } catch (e) { showGate("error", String(e)); }
  };
  $("btn-back").onclick = () => go(S.folders[S.cur]?.parent);
  $("crumbs").onclick = (e) => { const b = e.target.closest("[data-f]"); if (b) go(+b.dataset.f); };
  $("seg-view").onclick = (e) => {
    const b = e.target.closest("button[data-view]");
    if (!b) return;
    S.view = b.dataset.view;
    S.cursor = -1;
    renderAll();
  };
  $("when").onchange = (e) => { S.whenDays = +e.target.value; S.cursor = -1; renderAll(); };
  let qTimer;
  $("q").oninput = (e) => {
    clearTimeout(qTimer);
    qTimer = setTimeout(() => { S.q = e.target.value.trim(); S.cursor = -1; $("scroller").scrollTop = 0; renderAll(); }, 90);
  };
  $("q").onkeydown = (e) => {
    if (e.key === "Escape") { e.target.value = ""; S.q = ""; renderAll(); $("scroller").focus(); }
    if (e.key === "ArrowDown" || e.key === "Enter") { e.preventDefault(); $("scroller").focus(); setCursor(0); }
  };
  $("btn-inspector").onclick = () => { S.inspector = !S.inspector; $("btn-inspector").classList.toggle("on", S.inspector); renderInspector(); };
  $("btn-dest").onclick = chooseDestination;
  $("btn-overview").onclick = () => { if (!O.scanning && S.items && S.items.length) overviewFromSnapshot(); showOverview(); };
  $("btn-browse").onclick = () => { showMain(); go(0); };
  $("ov-rows").onclick = onOverviewClick;
  $("btn-restore").onclick = confirmRestore;
  $("btn-keys").onclick = showKeys;
  $("btn-clear").onclick = () => selectAllShownGlobal(false);
  $("check-all").onclick = () => { const on = $("check-all").classList.contains("on"); selectAllShown(!on); };
  $("btn-menu").onclick = () =>
    openMenu($("btn-menu"), [
      { label: "Scan again", sub: "Reads the whole list from Apple again", run: () => ask("Scan again?", "Salvage asks Apple for the whole list again. This can take a long time if you have many deleted files.", "Scan Again", () => scan(true)) },
      { label: "Keyboard shortcuts", sub: "Press ? at any time", run: showKeys },
      "-",
      { label: "Sign out", sub: "Removes the session and cache from this Mac", run: () => ask("Sign out?", "Salvage removes your sign-in and the saved list from this Mac. Next time, it must scan again.", "Sign Out", () => invoke("sign_out"), true) },
    ], { above: true });

  document.querySelectorAll(".thead .sortable").forEach((h) => {
    h.onclick = () => {
      const k = h.dataset.sort;
      S.sort = S.sort.key === k ? { key: k, dir: -S.sort.dir } : { key: k, dir: k === "name" || k === "loc" ? 1 : k === "exp" ? 1 : -1 };
      renderAll();
    };
  });

  $("tree").onclick = (e) => {
    const row = e.target.closest(".tree-row");
    if (!row) return;
    const f = +row.dataset.f;
    const act = e.target.closest("[data-act]")?.dataset.act;
    if (act === "disc") {
      S.expanded.has(f) ? S.expanded.delete(f) : S.expanded.add(f);
      renderTree();
    } else if (act === "check") {
      toggleFolder(f);
      refreshSelection();
    } else {
      if (S.cur === f && S.children[f].length) { S.expanded.has(f) ? S.expanded.delete(f) : S.expanded.add(f); }
      go(f);
    }
  };
  $("kinds").onclick = (e) => {
    const row = e.target.closest("[data-ext]");
    if (!row) return;
    const ext = row.dataset.ext;
    const next = new Set(S.hide);
    if (e.altKey) {
      // show only this type: hide every other type in this folder
      for (const [e] of kindsHere()) next.add(e);
      next.delete(ext);
    } else if (next.has(ext)) next.delete(ext);
    else next.add(ext);
    setHide(next);
  };
  $("kinds-all").onclick = () => {
    const next = new Set(S.hide);
    for (const [e] of kindsHere()) next.delete(e);
    setHide(next);
  };
  $("kinds-none").onclick = () => {
    const next = new Set(S.hide);
    for (const [e] of kindsHere()) next.add(e);
    setHide(next);
  };

  let treeRaf = 0;
  $("tree-scroll").addEventListener("scroll", () => {
    if (treeRaf) return;
    treeRaf = requestAnimationFrame(() => { treeRaf = 0; renderTree(false); });
  }, { passive: true });
  new ResizeObserver(() => renderTree(false)).observe($("tree-scroll"));
  $("kinds-q").oninput = () => renderKinds();

  const sc = $("scroller");
  sc.addEventListener("scroll", () => renderRows(false), { passive: true });
  new ResizeObserver(() => renderRows(true)).observe(sc);
  $("rows").addEventListener("mousedown", (e) => {
    const rowEl = e.target.closest(".row");
    if (!rowEl) return;
    const r = +rowEl.dataset.r;
    if (e.target.closest("[data-act=check]")) {
      e.preventDefault();
      if (e.shiftKey && S.anchor >= 0) {
        const on = rowIsOn(S.anchor);
        const [a, b] = [Math.min(S.anchor, r), Math.max(S.anchor, r)];
        for (let k = a; k <= b; k++) toggleRow(k, on);
      } else {
        toggleRow(r);
        S.anchor = r;
      }
      S.cursor = r;
      refreshSelection();
      sc.focus();
      return;
    }
    S.anchor = r;
    setCursor(r, false);
  });
  $("rows").addEventListener("dblclick", (e) => {
    const rowEl = e.target.closest(".row");
    if (!rowEl || e.target.closest("[data-act]")) return;
    const row = S.rows[+rowEl.dataset.r];
    if (row.t === "f") go(row.i);
    else { toggleRow(+rowEl.dataset.r); refreshSelection(); }
  });

  document.addEventListener("keydown", (e) => {
    const cmd = e.metaKey || e.ctrlKey;
    if (!$("menu").hidden && e.key === "Escape") { $("menu").hidden = true; return; }
    if (modalOpen()) {
      if (e.key === "Escape" && !S.job) closeModal();
      return;
    }
    if ($("main").hidden) return;
    const inField = e.target.tagName === "INPUT" || e.target.tagName === "SELECT";
    if (cmd && e.key === "f") { e.preventDefault(); $("q").focus(); $("q").select(); return; }
    if (cmd && e.key === "i") { e.preventDefault(); $("btn-inspector").click(); return; }
    if (cmd && e.key === "r") { e.preventDefault(); confirmRestore(); return; }
    if (inField) return;
    if (e.key === "?") { e.preventDefault(); showKeys(); return; }
    if (cmd && e.key === "a") { e.preventDefault(); selectAllShown(!e.shiftKey); return; }
    if (cmd && e.key === "ArrowUp") { e.preventDefault(); go(S.folders[S.cur].parent); return; }
    switch (e.key) {
      case "ArrowDown": e.preventDefault(); setCursor(S.cursor + 1); break;
      case "ArrowUp": e.preventDefault(); setCursor(S.cursor - 1); break;
      case "PageDown": e.preventDefault(); setCursor(S.cursor + Math.floor(sc.clientHeight / ROW_H)); break;
      case "PageUp": e.preventDefault(); setCursor(S.cursor - Math.floor(sc.clientHeight / ROW_H)); break;
      case "Home": e.preventDefault(); setCursor(0); break;
      case "End": e.preventDefault(); setCursor(S.rows.length - 1); break;
      case " ": e.preventDefault(); if (S.cursor >= 0) { toggleRow(S.cursor); refreshSelection(); } break;
      case "Enter": case "ArrowRight": {
        const row = S.rows[S.cursor];
        if (row && row.t === "f") { e.preventDefault(); go(row.i); }
        break;
      }
      case "Backspace": case "ArrowLeft": e.preventDefault(); if (S.cur) { const from = S.cur; go(S.folders[S.cur].parent); const r = S.rows.findIndex((x) => x.t === "f" && x.i === from); if (r >= 0) setCursor(r); } break;
      case "Escape": if (S.q || S.whenDays) { S.q = ""; S.whenDays = 0; $("q").value = ""; $("when").value = "0"; renderAll(); } else selectAllShownGlobal(false); break;
    }
  });
}

function selectAllShownGlobal(on) {
  if (!S.sel) return;
  for (let i = 0; i < S.sel.length; i++) if (S.sel[i] !== (on ? 1 : 0)) setSel(i, on);
  refreshSelection();
}

// ---------------- boot ----------------
async function boot() {
  invoke("mark", { stage: "script" });
  wire();
  // Register all listeners at once (one round trip each, in parallel).
  await Promise.all([
    listen("auth://changed", (account) => {
      if (account) signedIn(account); // keeps the saved scan; only "Scan again" asks Apple again
      else { S.account = null; S.items = []; S.sel = null; showGate("signin"); }
    }),
    listen("scan://progress", (p) => {
      O.progress = p.phase === "paths"
        ? `Finding folder names${p.total ? ` (${n(p.done)} of ${n(p.total)})` : ""}…`
        : `Apple has sent ${plural(p.count, "file")} so far…`;
      if (!$("gate").hidden) $("gate-progress-text").textContent = O.progress;
      renderOvState();
    }),
    listen("scan://folders", (p) => {
      O.folders = p.folders;
      O.files = p.files;
      O.bytes = p.bytes;
      O.complete = false; // the snapshot load sets this when the scan returns
      O.map = null;
      scheduleOverview();
    }),
    listen("restore://progress", onProgress),
    listen("copies://progress", (p) => {
      O.progress = `Checking for exact copies (${n(p.done)} of ${n(p.total)})…`;
      renderOvState();
    }),
  ]);
  setInterval(renderAccount, 60000);
  const st = await invoke("app_state");
  if (st.account) return signedIn(st.account);
  // Show the last saved scan at once. Check the saved sign-in at the same time.
  const resume = invoke("resume_session");
  // Paint the folder totals first (small JSON). Load the full file list after.
  const ov = await invoke("get_overview");
  let full = null;
  if (ov) {
    O.folders = ov.folders;
    O.files = ov.files;
    O.bytes = ov.bytes;
    O.complete = true;
    O.scanning = false;
    O.map = null;
    // Wait for the first paint, so the big list does not delay it.
    O.loading = afterPaint().then(() => invoke("get_snapshot")).then((v) => {
      if (v) { load(v); overviewFromSnapshot(); }
      O.loading = null;
      renderOverview();
      return v;
    });
    full = O.loading;
    showOverview();
  } else showGate("busy", "Checking for a saved sign-in…");
  shown();
  const acc = await resume;
  if (full) await full;
  if (!acc) { S.items = []; S.sel = null; showGate("signin"); return; }
  if (ov && ov.dsid === acc.dsid) {
    S.account = acc;
    renderAccount();
    checkCopies();
    return;
  }
  signedIn(acc);
}

// Resolves after the next frame is on screen.
const afterPaint = () => new Promise((r) => requestAnimationFrame(() => setTimeout(r, 0)));

// Log once when the first useful screen is painted (startup timing).
let shownDone = false;
function shown() {
  if (shownDone) return;
  shownDone = true;
  afterPaint().then(() => invoke("mark", { stage: "shown" }));
}

boot().catch((e) => showGate("error", String(e)));
