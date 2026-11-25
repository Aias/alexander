#!/usr/bin/env bash
# Build Alexander and assemble "studio/target/Alexander.app".
#
#   studio/macos/bundle.sh               release build for this Mac's architecture
#   studio/macos/bundle.sh --universal   arm64 + x86_64 joined with lipo
#   studio/macos/bundle.sh --dmg         also write a compressed disk image
#
# The bundle is signed ad hoc only: enough for Apple silicon to run it locally,
# not enough for Gatekeeper on another Mac without a right-click > Open.

set -euo pipefail

die() {
	printf 'bundle.sh: %s\n' "$*" >&2
	exit 1
}

usage() {
	sed -n '2,7s/^# \{0,1\}//p' "$0"
}

universal=0
dmg=0
for arg in "$@"; do
	case "$arg" in
		--universal) universal=1 ;;
		--dmg) dmg=1 ;;
		-h | --help)
			usage
			exit 0
			;;
		*) die "unknown option: $arg (try --help)" ;;
	esac
done

os="$(uname -s)"
[[ "$os" == Darwin ]] || die "this script needs macOS (codesign, sips, iconutil, lipo); this is $os"

for tool in cargo codesign sips iconutil plutil; do
	command -v "$tool" > /dev/null || die "required tool not found: $tool"
done
if ((universal)); then
	command -v lipo > /dev/null || die "required tool not found: lipo"
fi

# Resolve paths from the script itself so it works from any directory.
macos_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
studio_dir="$(dirname "$macos_dir")"
target_dir="$studio_dir/target"
# Keep cargo's output where this script looks for it even if the caller's
# environment points elsewhere.
export CARGO_TARGET_DIR="$target_dir"

app_name="Alexander"
bin_name="alexander-studio"
app="$target_dir/$app_name.app"

cd "$studio_dir"

# `cargo pkgid` ends in "#<version>" or "#<name>@<version>" depending on the
# cargo release; strip whichever prefix is there.
pkgid="$(cargo pkgid --offline 2> /dev/null || cargo pkgid)"
version="${pkgid##*#}"
version="${version##*@}"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+ ]] || die "could not read the package version from: $pkgid"

if ((universal)); then
	targets=(aarch64-apple-darwin x86_64-apple-darwin)
	if command -v rustup > /dev/null; then
		rustup target add "${targets[@]}"
	fi
	binaries=()
	for t in "${targets[@]}"; do
		cargo build --release --locked --bin "$bin_name" --target "$t"
		binaries+=("$target_dir/$t/release/$bin_name")
	done
	binary="$target_dir/universal/$bin_name"
	mkdir -p "$(dirname "$binary")"
	lipo -create -output "$binary" "${binaries[@]}"
else
	cargo build --release --locked --bin "$bin_name"
	binary="$target_dir/release/$bin_name"
fi
[[ -x "$binary" ]] || die "build finished but $binary is missing"

# Start from an empty bundle so files from an earlier layout never linger.
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/$bin_name"

sed "s/@VERSION@/$version/g" "$macos_dir/Info.plist" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" > /dev/null || die "Info.plist does not validate"
printf 'APPL????' > "$app/Contents/PkgInfo"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
iconset="$work/AppIcon.iconset"
mkdir "$iconset"
# iconutil wants exactly these names; each size has a 1x and a 2x image.
for size in 16 32 128 256 512; do
	double=$((size * 2))
	sips -z "$size" "$size" "$macos_dir/icon.png" --out "$iconset/icon_${size}x${size}.png" > /dev/null
	sips -z "$double" "$double" "$macos_dir/icon.png" --out "$iconset/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil --convert icns --output "$app/Contents/Resources/AppIcon.icns" "$iconset"

codesign --force --deep -s - "$app"
codesign --verify --strict "$app" || die "the signed bundle does not verify"

if ((dmg)); then
	dmg_path="$target_dir/$app_name $version.dmg"
	stage="$work/dmg"
	mkdir "$stage"
	cp -R "$app" "$stage/"
	# An Applications link lets people install by dragging inside the window.
	ln -s /Applications "$stage/Applications"
	rm -f "$dmg_path"
	hdiutil create -quiet -volname "$app_name" -srcfolder "$stage" -format UDZO -ov "$dmg_path"
	printf '%s\n' "$dmg_path"
fi

printf '%s\n' "$app"
