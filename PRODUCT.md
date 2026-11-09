# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

(A Tauri 2 macOS app. The UI is plain HTML, CSS and JS in a WKWebView, in a native window with an overlay title bar and sidebar vibrancy. It must feel like a native Mac app.)

## Users

Anyone who deleted iCloud Drive files and needs them back. They are often stressed, sometimes not technical, and short on time: Apple deletes the files for good about 30 days after deletion. Confirmed: calm and clear matters more than density.

## Product Purpose

Salvage lists every deleted iCloud Drive file in its real folder tree and restores what the user picks: where it was, to another iCloud Drive folder, or downloaded to a folder on the Mac. Success: the user sees that all their files are found, picks them with confidence, and gets them back with zero failures.

## Positioning

Apple's own "Restore Files" page on icloud.com has no folders and breaks with thousands of files. Salvage shows deleted files in their folders, handles tens of thousands of files, retries when Apple is busy, hides exact copies and remembers what it already restored. It was built to save 57 GB; one run restored 58,013 files with 0 failures.

## Operating Context

Flow: sign in on Apple's own page inside the app, scan (the list streams in, can take minutes), overview of deleted files by folder, browse and pick (folder tree, file list, file types filter, details panel), restore with live progress. A saved scan opens instantly on the next launch.

## Capabilities and Constraints

- Runs only on the Mac. Talks only to Apple. Strict CSP: no outside fonts, scripts or images; everything ships in `ui/`.
- Vanilla JS with no build step. Lists can hold 60,000+ items, so rows must stay cheap to render.
- Window uses `titleBarStyle: Overlay` with traffic lights at x 18, y 22, and macOS sidebar vibrancy.
- Light and dark appearance, following the macOS setting (confirmed).
- Not made by or affiliated with Apple. Must not imitate Apple branding.

## Brand Commitments

- Name: Salvage. App icon: `src-tauri/icons/source.svg` (blue tile, cloud with an up arrow).
- Voice: plain, short, honest sentences (Simplified Technical English). No hype.
- Look: a native, premium Mac app with its own identity in detail, not a custom-branded world (confirmed).

## Evidence on Hand

- README numbers: 57 GB saved; 58,013 files restored in one run with 0 failures.
- Screenshots in `docs/screenshots/` use made-up example files and a made-up account.
- No testimonials, users or press. Do not invent them.

## Product Principles

1. Calm under pressure: the user is worried about losing files; every screen says what is safe and what is next.
2. Show the whole truth: counts, sizes and "gone in N days" are always visible and exact.
3. Native first: macOS habits (sidebar, toolbar, selection, keyboard) work as expected.
4. Fast with huge lists: nothing in the design may slow down 60,000 rows.
5. Privacy is visible: it is clear that only Apple sees the user's data.

## Accessibility & Inclusion

Keyboard use of the list and tree, visible focus, readable contrast in both appearances, and respect for Reduce Motion.
