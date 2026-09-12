#!/usr/bin/env bash
set -euo pipefail

root_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
output_dir="${1:-$root_dir/target/release/bundle/deb}"
manifest="$root_dir/Cargo.toml"
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$manifest" | head -n 1)"
architecture="$(dpkg --print-architecture)"

if [[ -z "$version" ]]; then
  printf '%s\n' "Could not read the GPUI package version from $manifest" >&2
  exit 1
fi

if [[ "$architecture" != "amd64" ]]; then
  printf '%s\n' "Only amd64 packaging is currently supported; found $architecture." >&2
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

install -Dm755 "$root_dir/target/release/vintage-gpui" "$staging_dir/usr/bin/vintage-gpui"
install -Dm644 "$root_dir/packaging/gpui/dev.tiebi.vintage.gpui.desktop" \
  "$staging_dir/usr/share/applications/dev.tiebi.vintage.gpui.desktop"
install -Dm644 "$root_dir/crates/vintage-gpui/assets/favicon.svg" \
  "$staging_dir/usr/share/icons/hicolor/scalable/apps/dev.tiebi.vintage.gpui.svg"
install -Dm644 "$root_dir/packaging/gpui/control.in" "$staging_dir/DEBIAN/control.in"
sed \
  -e "s/@VERSION@/$version/g" \
  -e "s/@ARCHITECTURE@/$architecture/g" \
  "$staging_dir/DEBIAN/control.in" > "$staging_dir/DEBIAN/control"
rm "$staging_dir/DEBIAN/control.in"

mkdir -p "$output_dir"
package_path="$output_dir/VINTAGE_${version}_${architecture}.deb"
dpkg-deb --build --root-owner-group "$staging_dir" "$package_path" >/dev/null
printf '%s\n' "$package_path"
