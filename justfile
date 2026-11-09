# Salvage tasks. Run `just` to see them.

repo := "haim-ari/salvage"
version := `sed -n 's/^  "version": "\(.*\)",$/\1/p' src-tauri/tauri.conf.json`
arch := `uname -m`
dmg_file := "dist/Salvage-" + version + "-macos-" + arch + ".dmg"

alias DMG := dmg

# List the tasks.
default:
    @just --list

# Install the build tools.
install:
    bun install

# Build Salvage.app.
app: install
    bun run build

# Build the app and pack it into a DMG in dist/. No release.
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

# Build the DMG and publish it as a public GitHub release v<version>.
dmg: check-release dmg-file
    #!/usr/bin/env bash
    set -euo pipefail
    tag="v{{version}}"
    if gh release view "$tag" --repo {{repo}} >/dev/null 2>&1; then
        echo "Release $tag exists. Replacing its DMG."
        gh release upload "$tag" "{{dmg_file}}" --repo {{repo}} --clobber
    else
        gh release create "$tag" "{{dmg_file}}" --repo {{repo}} \
            --target "$(git rev-parse HEAD)" --title "Salvage {{version}}" \
            --generate-notes --notes "$(printf '%s\n' \
                'Download the DMG below. Open it and drag Salvage to Applications.' '' \
                'The app is not signed by Apple. To open it the first time: right-click Salvage.app, pick Open, then Open again. See the README for more.')"
    fi
    gh release view "$tag" --repo {{repo}} --json url -q .url

# Stop a release that would not match the code on GitHub.
[private]
check-release:
    #!/usr/bin/env bash
    set -euo pipefail
    [ -n "{{version}}" ] || { echo "No version in src-tauri/tauri.conf.json"; exit 1; }
    [ -z "$(git status --porcelain)" ] || { echo "Commit your changes first."; exit 1; }
    git fetch -q origin
    git merge-base --is-ancestor HEAD origin/main || { echo "Push this commit to main first."; exit 1; }
    [ "$(gh repo view {{repo}} --json visibility -q .visibility)" = PUBLIC ] \
        || { echo "{{repo}} is not public."; exit 1; }
