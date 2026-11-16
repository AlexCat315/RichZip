#!/usr/bin/env bash
# Builds the two macOS bundles MugenZip ships as:
#
#   MugenZip.app        the browser (owns archives, `mugenzip://` URLs)
#   MugenZipWidget.app  the menu-bar dot helper (LSUIElement, `--widget`)
#
# They must be separate bundles. Sharing one bundle id makes LaunchServices
# route every URL and document open to whichever instance happens to be alive,
# so "Settings" in the dot would wake the dot instead of the browser.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cove_root="${COVE_ROOT:-$root/../Cove}"
out="${1:-$root/target/mac-app}"
stage="$(mktemp -d /tmp/mugenzip-package.XXXXXX)"
trap 'rm -rf "$stage"' EXIT

cd "$root"
cargo build
"$cove_root/target/debug/cove-package" package mac --output "$stage"

rm -rf "$out/MugenZip.app" "$out/MugenZipWidget.app"
mkdir -p "$out"
ditto "$stage/MugenZip.app" "$out/MugenZip.app"
ditto "$stage/MugenZip.app" "$out/MugenZipWidget.app"

# The helper is the same executable with its own identity: no Dock tile, no
# document/URL claims, only the status item and the floating window.
plist="$out/MugenZipWidget.app/Contents/Info.plist"
plutil -replace CFBundleIdentifier -string com.mugenzip.widget "$plist"
plutil -replace CFBundleName -string "MugenZipWidget" "$plist"
plutil -replace CFBundleDisplayName -string "MugenZip 悬浮菜单" "$plist"
plutil -remove CFBundleDocumentTypes "$plist" 2>/dev/null || true
plutil -remove CFBundleURLTypes "$plist" 2>/dev/null || true
if plutil -extract LSUIElement raw "$plist" >/dev/null 2>&1; then
    plutil -replace LSUIElement -bool true "$plist"
else
    plutil -insert LSUIElement -bool true "$plist"
fi

codesign --force --deep --sign - "$out/MugenZip.app"
codesign --force --deep --sign - "$out/MugenZipWidget.app"
codesign --verify --strict "$out/MugenZip.app"
codesign --verify --strict "$out/MugenZipWidget.app"

lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
"$lsregister" -f "$out/MugenZip.app"
"$lsregister" -f "$out/MugenZipWidget.app"

printf 'MugenZip.app        %s\n' "$out/MugenZip.app"
printf 'MugenZipWidget.app  %s\n' "$out/MugenZipWidget.app"
