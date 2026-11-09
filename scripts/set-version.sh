#!/usr/bin/env bash
# Set the app version in every file that has it. Used by the release job.
# The version is not committed back: the git tag is the source of truth.
set -euo pipefail
v="${1:?usage: set-version.sh X.Y.Z}"
[[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "bad version: $v" >&2; exit 1; }
cd "$(dirname "$0")/.."
# first match only (no /g), so dependency versions stay as they are
perl -0pi -e "s/^  \"version\": \"[^\"]*\",\$/  \"version\": \"$v\",/m" package.json src-tauri/tauri.conf.json
perl -0pi -e "s/^version = \"[^\"]*\"\$/version = \"$v\"/m" src-tauri/Cargo.toml
grep -q "^  \"version\": \"$v\",$" package.json
grep -q "^  \"version\": \"$v\",$" src-tauri/tauri.conf.json
grep -q "^version = \"$v\"$" src-tauri/Cargo.toml
echo "version $v"
