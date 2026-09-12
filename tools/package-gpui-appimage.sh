#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${1:-$root_dir/target/release/bundle/appimage}"
manifest="$root_dir/Cargo.toml"
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$manifest" | head -n 1)"

if [[ -z "$version" ]]; then
  printf '%s\n' "Could not read the GPUI package version from $manifest" >&2
  exit 1
fi

if [[ "$(uname -m)" != "x86_64" ]]; then
  printf '%s\n' "Only x86_64 AppImage packaging is currently supported." >&2
  exit 1
fi

native_lib_dir="$root_dir/.data/native-libs"
if [[ -f "$native_lib_dir/libxkbcommon-x11.so" ]]; then
  LIBRARY_PATH="${LIBRARY_PATH:+$LIBRARY_PATH:}$native_lib_dir" \
    cargo +1.95.0 build --manifest-path "$manifest" -p vintage-gpui --release --locked
else
  cargo +1.95.0 build --manifest-path "$manifest" -p vintage-gpui --release --locked
fi

staging_dir="$(mktemp -d)"
cleanup() {
  rm -rf "$staging_dir"
}
trap cleanup EXIT

app_dir="$staging_dir/VINTAGE.AppDir"
install -Dm755 "$root_dir/target/release/vintage-gpui" "$app_dir/usr/bin/vintage-gpui"
install -Dm644 "$root_dir/packaging/gpui/dev.tiebi.vintage.gpui.desktop" \
  "$app_dir/dev.tiebi.vintage.gpui.desktop"
install -Dm644 "$root_dir/crates/vintage-gpui/assets/favicon.svg" \
  "$app_dir/dev.tiebi.vintage.gpui.svg"
ln -s usr/bin/vintage-gpui "$app_dir/AppRun"

tool_dir="${XDG_CACHE_HOME:-$HOME/.cache}/vintage/appimagetool"
tool_path="$tool_dir/appimagetool-x86_64.AppImage"
if [[ ! -x "$tool_path" ]]; then
  mkdir -p "$tool_dir"
  curl --fail --location --silent --show-error \
    --output "$tool_path" \
    https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
  chmod +x "$tool_path"
fi

mkdir -p "$output_dir"
output_path="$output_dir/VINTAGE_${version}_amd64.AppImage"
ARCH=x86_64 "$tool_path" --appimage-extract-and-run "$app_dir" "$output_path" >/dev/null
printf '%s\n' "$output_path"
