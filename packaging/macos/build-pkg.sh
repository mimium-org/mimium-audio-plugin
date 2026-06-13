#!/usr/bin/env bash
set -euo pipefail

version=${1:?usage: build-pkg.sh <version>}
script_dir=$(cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(cd -- "$script_dir/../.." && pwd)
package_dir="$repo_root/target/package/release"
out_dir="$repo_root/target/installer/macos"
root_dir="$out_dir/root"
scripts_dir="$out_dir/scripts"
pkg_path="$out_dir/Mimium-Audio-Plugin-${version}-macOS.pkg"

find_first() {
  find "$package_dir" -maxdepth 1 -name "$1" -print -quit
}

clap_path=$(find_first '*.clap')
vst3_path=$(find_first '*.vst3')
au_path=$(find_first '*.component')

if [[ -z "$clap_path" || -z "$vst3_path" || -z "$au_path" ]]; then
  echo "expected .clap, .vst3 and .component artifacts in $package_dir" >&2
  exit 1
fi

rm -rf "$root_dir" "$scripts_dir" "$pkg_path"
mkdir -p \
  "$root_dir/Library/Audio/Plug-Ins/CLAP" \
  "$root_dir/Library/Audio/Plug-Ins/VST3" \
  "$root_dir/Library/Audio/Plug-Ins/Components" \
  "$scripts_dir" \
  "$out_dir"

cp "$script_dir/scripts/postinstall" "$scripts_dir/postinstall"
chmod +x "$scripts_dir/postinstall"

ditto "$clap_path" "$root_dir/Library/Audio/Plug-Ins/CLAP/$(basename "$clap_path")"
ditto "$vst3_path" "$root_dir/Library/Audio/Plug-Ins/VST3/$(basename "$vst3_path")"
ditto "$au_path" "$root_dir/Library/Audio/Plug-Ins/Components/$(basename "$au_path")"

sign_bundle() {
  local path=$1
  /usr/bin/codesign --force --sign "$APPLE_APPLICATION_SIGN_IDENTITY" --timestamp --options runtime --deep "$path"
}

if [[ -n "${APPLE_APPLICATION_SIGN_IDENTITY:-}" ]]; then
  sign_bundle "$root_dir/Library/Audio/Plug-Ins/CLAP/$(basename "$clap_path")"
  sign_bundle "$root_dir/Library/Audio/Plug-Ins/VST3/$(basename "$vst3_path")"
  sign_bundle "$root_dir/Library/Audio/Plug-Ins/Components/$(basename "$au_path")"
fi

pkgbuild_args=(
  --root "$root_dir"
  --scripts "$scripts_dir"
  --identifier org.mimium.mimium-audio-plugin.pkg
  --version "$version"
  --install-location /
)

if [[ -n "${APPLE_INSTALLER_SIGNING_IDENTITY:-}" ]]; then
  pkgbuild_args+=(--sign "$APPLE_INSTALLER_SIGNING_IDENTITY")
fi

pkgbuild_args+=("$pkg_path")
pkgbuild "${pkgbuild_args[@]}"

if [[ -n "${APPLE_INSTALLER_SIGNING_IDENTITY:-}" && -n "${APPLE_NOTARY_APPLE_ID:-}" && -n "${APPLE_NOTARY_TEAM_ID:-}" && -n "${APPLE_NOTARY_PASSWORD:-}" ]]; then
  xcrun notarytool submit "$pkg_path" --apple-id "$APPLE_NOTARY_APPLE_ID" --team-id "$APPLE_NOTARY_TEAM_ID" --password "$APPLE_NOTARY_PASSWORD" --wait
  xcrun stapler staple "$pkg_path"
fi

echo "Built PKG: $pkg_path"
