# Salvage

Get your deleted iCloud Drive files back, fast.

Apple keeps deleted iCloud Drive files for about 30 days. The only official way to get them back is the "Restore Files" page on icloud.com. With thousands of files, that page is slow, it has no folders, and it is easy to miss things.

Salvage is a small native Mac app that does the same job better:

- It shows every deleted file in its folder tree, with size and dates.
- Tick folders or files, or filter by file type.
- It restores them where they were, to another iCloud Drive folder, or downloads them to a folder on your Mac.
- It finds duplicate copies and hides them.
- It handles tens of thousands of files. When Apple is busy, it waits and tries again.
- It remembers what it already restored, so a second run only does what is left.

It was built to save 57 GB of files that the web page could not handle. In one run it restored 58,013 files with 0 failures.

## How it works

1. You sign in on Apple's own sign-in page, inside the app. Salvage never sees your password.
2. Salvage reads the list of deleted files from the same iCloud web service that icloud.com uses.
3. You pick what to restore. Salvage asks Apple to bring the files back, then moves or downloads them if you asked for that.

Everything runs on your Mac. Salvage talks only to Apple. It sends nothing anywhere else.

Its own data (the file list and what was restored) is kept in `~/Library/Application Support/io.github.haim-ari.salvage/`.

## Important

- Salvage is not made by Apple and is not supported by Apple. It uses the same private web service as icloud.com. Apple can change that service at any time, and then Salvage can stop working.
- Use it at your own risk. It is MIT licensed, with no warranty.
- Restored files take space in your iCloud storage. Check that you have enough free space first.

## Build

You need macOS 13 or later, [Rust](https://rustup.rs) 1.90 or later, and [bun](https://bun.sh).

```bash
bun install
bun run build
```

The app is in `src-tauri/target/release/bundle/macos/Salvage.app`.

To run it while you work on it:

```bash
bun run dev
```

## Code layout

- `src-tauri/src/api.rs`: the iCloud web service calls (list, recover, move, download).
- `src-tauri/src/auth.rs`: sign-in in a web view, reading the session cookies.
- `src-tauri/src/model.rs`: the snapshot of deleted files and folders.
- `src-tauri/src/copies.rs`: finds duplicate copies.
- `src-tauri/src/restore.rs`: the restore job, with batches, retries and progress.
- `src-tauri/src/store.rs`: what is saved on disk.
- `ui/`: the interface (plain HTML, CSS and JavaScript).

## License

[MIT](LICENSE)
