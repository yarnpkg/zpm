#!/usr/bin/env bash
set -euo pipefail

version=3.0.1
toolchain=browserpod-$version
base_url=https://rt.browserpod.io/$version/rust
tarball=browserpod-rust-$version.tar.gz
profile=release-lto-nodebug
target=wasm32-browserpod-linux-musl

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd -- "$script_dir/../.." && pwd)
tmp=$(mktemp -d)
lock_backup="$tmp/Cargo.lock"
cp "$repo_root/Cargo.lock" "$lock_backup"

cleanup() {
  cp "$lock_backup" "$repo_root/Cargo.lock"
  rm -rf "$tmp"
}

trap cleanup EXIT INT TERM

if command -v brew >/dev/null 2>&1; then
  llvm_prefix=$(brew --prefix llvm 2>/dev/null || true)

  if [[ -n "$llvm_prefix" && -d "$llvm_prefix/bin" ]]; then
    export PATH="$llvm_prefix/bin:$PATH"
    export BP_CLANG="$llvm_prefix/bin/clang"
    export BP_LLVM_AR="$llvm_prefix/bin/llvm-ar"
  fi
fi

if ! rustup toolchain list | grep -q "^$toolchain"; then
  curl -fsSL "$base_url/install.sh" -o "$tmp/install.sh"
  curl -fsSL "$base_url/$tarball" -o "$tmp/$tarball"

  BP_DIST_BASE=$base_url sh "$tmp/install.sh" "$toolchain"
fi

# Pin libc to the version shipped in the BrowserPod sysroot, mirroring the
# CI build action, so local builds match CI instead of failing on a newer libc.
sysroot=$(rustc "+$toolchain" --print sysroot)
libc_manifest="$sysroot/lib/browserpod-libc/Cargo.toml"
libc_version=$(awk -F '"' '/^version = / { print $2; exit }' "$libc_manifest")

cargo "+$toolchain" update -p libc --precise "$libc_version"

cargo "+$toolchain" build \
  --bin yarn-bin \
  --profile "$profile" \
  --target "$target"

asset_dir="$repo_root/website/public/browserpod"
mkdir -p "$asset_dir"
cp "$repo_root/target/$target/$profile/yarn-bin" "$asset_dir/yarn-bin.wasm"

cat > "$asset_dir/manifest.json" <<JSON
{
  "browserpod": "$version",
  "rustToolchain": "$toolchain",
  "source": "$base_url/$tarball",
  "target": "$target",
  "profile": "$profile",
  "binary": "yarn-bin.wasm"
}
JSON

echo "Wrote $asset_dir/yarn-bin.wasm"
