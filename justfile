# Salvage tasks. Run `just` to see them.

version := `sed -n 's/^  "version": "\(.*\)",$/\1/p' src-tauri/tauri.conf.json`
arch := `uname -m`
dmg_file := "dist/Salvage-" + version + "-macos-" + arch + ".dmg"

# List the tasks.
default:
    @just --list

# Install the build tools.
install:
    bun install

# Build Salvage.app.
app: install
    bun run build

# Build the app and pack it into a DMG in dist/.
# The release job runs this. Releases are made by CI on merge to main.
dmg-file: app
    #!/usr/bin/env bash
    set -euo pipefail
    stage="$(mktemp -d)"
    trap 'rm -rf "$stage"' EXIT
    ditto src-tauri/target/release/bundle/macos/Salvage.app "$stage/Salvage.app"
    ln -s /Applications "$stage/Applications"
    mkdir -p dist
    rm -f "{{dmg_file}}"
    hdiutil create -quiet -volname "Salvage {{version}}" -srcfolder "$stage" \
        -fs HFS+ -format UDZO -ov "{{dmg_file}}"
    hdiutil verify -quiet "{{dmg_file}}"
    echo "DMG: {{dmg_file}}"
