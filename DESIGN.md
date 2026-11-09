---
name: Salvage
description: Native Mac app to find and restore deleted iCloud Drive files. Swiss timetable look.
colors:
  accent: "#1f6bf0"
  accent-fill: "#1f6bf0"
  accent-ink: "#ffffff"
  content: "#fbfbfa"
  content-2: "#f4f4f2"
  raised: "#ffffff"
  text: "#16171a"
  text-2: "#55585f"
  text-3: "#74777e"
  line: "rgba(22, 23, 26, 0.075)"
  line-2: "rgba(22, 23, 26, 0.13)"
  signal: "#c2410c"
  ok: "#1a7f4b"
  danger: "#d1242f"
  clock: "#a9acb3"
  dark-accent: "#4b8dff"
  dark-accent-fill: "#2a6be6"
  dark-content: "#151618"
  dark-raised: "#222326"
  dark-text: "#ececee"
  dark-signal: "#ff9a5a"
typography:
  display:
    fontFamily: "-apple-system, BlinkMacSystemFont, SF Pro Text, Helvetica Neue, sans-serif"
    fontSize: "28px"
    fontWeight: 700
    lineHeight: 1.1
    letterSpacing: "-0.028em"
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, SF Pro Text, Helvetica Neue, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    lineHeight: 1.38
    letterSpacing: "-0.003em"
  label:
    fontFamily: "-apple-system, BlinkMacSystemFont, SF Pro Text, Helvetica Neue, sans-serif"
    fontSize: "11px"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "0.01em"
rounded:
  xs: "4px"
  sm: "6px"
  md: "7px"
  lg: "10px"
  xl: "14px"
spacing:
  row: "30px"
  sm: "8px"
  md: "14px"
  lg: "24px"
components:
  button-primary:
    backgroundColor: "{colors.accent-fill}"
    textColor: "{colors.accent-ink}"
    rounded: "{rounded.md}"
    padding: "6px 14px"
  button-secondary:
    backgroundColor: "{colors.line}"
    textColor: "{colors.text}"
    rounded: "{rounded.md}"
    padding: "6px 14px"
  table-row:
    height: "{spacing.row}"
    textColor: "{colors.text}"
  sheet:
    backgroundColor: "{colors.raised}"
    rounded: "{rounded.xl}"
---

# Design System: Salvage

## Overview

Salvage looks like a Swiss train timetable. The grid is exact. Numbers line up. Thin lines (hairlines) split the rows. The app is calm, because the people who use it just lost files.

It is a native Mac app first. It follows macOS light and dark mode. It uses the system font and system materials. It has one blue for action and one orange for "this file is gone soon". Nothing else is loud.

## Colors

### Primary
- **Accent blue** (`#1f6bf0` light, `#4b8dff` dark): links, focus rings, checkboxes, the selected row.
- **Accent fill** (`#1f6bf0` light, `#2a6be6` dark): the background of filled buttons. It is deeper than the dark accent so white text keeps 4.5:1 contrast.

### Secondary
- **Signal orange** (`#c2410c` light, `#ff9a5a` dark): only for files with fewer than 8 days left. Never for decoration.

### Neutral
- Ground `#fbfbfa`, side panel `#f4f4f2`, raised surfaces (sheets, menus) `#ffffff`.
- Text in three steps: `#16171a`, `#55585f`, `#74777e`.
- Lines in two steps: `line` (row hairlines) and `line-2` (panel edges, header rule).
- Status: `ok` green for "All found", `danger` red only for errors.

### Named Rules
- **One accent, one signal.** A new color needs a reason a person can say in one sentence.
- **Color is never the only signal.** The clock ring and the "4 days" text carry the same meaning as the orange.

## Typography

System font only (SF Pro). Tabular figures are on for the whole app (`font-feature-settings: "tnum"`), so sizes, dates and counts line up in columns.

### Hierarchy
- **Display** 28–30px / 700 / tight tracking: one per screen (overview title, sign-in title).
- **Body** 13px / 400: rows, inspector, sheets.
- **Label** 11px / 600: column headers, section labels.
- **Footnote** 12px / `text-3`: status bar, hints.

## Layout

- Three panes in browse: side panel (folders + file types), file table, optional inspector (290px).
- Table rows are 30px. Columns are fixed grid tracks; numbers align right.
- The overview is one table with a size bar per folder, under a left-aligned title.
- The sign-in gate is a left-aligned column, 480px wide, centered in the window. No card.

## Elevation & Depth

Flat by default. Depth only for things that float over the table:
- **Selection bar** and **menus**: glass (`backdrop-filter: blur`) plus `shadow-md`.
- **Sheets**: `raised` surface plus `shadow-lg` over a light scrim.
- Panels split with a 0.5px hairline, never a shadow.

## Shapes

Small, quiet radii: 4px (badges, keys), 6–7px (buttons, fields, rows in menus), 10px (menus), 14px (sheets, selection bar). Rows have no radius. Circles only for dots, avatars and the clock ring.

## Components

### Buttons
- **Primary**: flat `accent-fill`, white text, a 1px soft shadow. Hover brightens 7%. Press scales to 0.98.
- **Secondary**: `field` tint, normal text.
- **Danger**: red text, or red fill only in a confirm sheet.

### Inputs / Fields
Tinted `field` background, no border, 7px radius, accent caret. Focus shows a 2px accent ring.

### Navigation
Side panel tree with checkboxes and right-aligned counts. Breadcrumb in the toolbar. View switch is a segmented control.

### Status
A small dot plus text ("All found", "Finding files"). Never a filled badge. A tiny spinner replaces the dot while work runs.

### Clock ring (signature)
Each file shows a 12px ring in "Gone in". The ring fills as the 30 days run out (`--p`, drawn with `conic-gradient`). Under 8 days it turns signal orange, and so does the text.

## Do's and Don'ts

### Do:
- Keep numbers in tabular figures and right-aligned.
- Split rows with hairlines, not zebra stripes.
- Use the signal orange only for "gone soon".
- Respect reduced motion: all animation drops to 1ms.

### Don't:
- Don't add gradients, glows or colored cards.
- Don't add a second accent color.
- Don't use emoji or text glyphs (✓ ▸) as icons; use the SVG icons.
- Don't put white text on the light dark-mode accent `#4b8dff`; use `accent-fill`.
