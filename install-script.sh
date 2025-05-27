#!/usr/bin/env bash
set -euo pipefail

error() {
    echo "error: $1" >&2
    exit 1
}

tildify() {
    if [[ $1 = $HOME/* ]]; then
        local replacement=\~/

        echo "${1/$HOME\//$replacement}"
    else
        echo "$1"
    fi
}

command -v unzip >/dev/null ||
    error 'unzip is required to install Yarn'

if [[ $# -gt 1 ]]; then
    error 'Too many arguments, only one representing a specific tag to install is allowed. (e.g. "6.0.0")'
fi

platform=$(uname -ms)

case $platform in
'Darwin arm64')
    target=aarch64-apple-darwin
    ;;
'Linux x86_64' | *)
    target=x86_64-unknown-linux-gnu
    ;;
esac

install_dir=$HOME/.yarn/switch/bin
tmp_dir=$install_dir.tmp
archive=$tmp_dir/yarn.zip

rm -rf "$tmp_dir"
mkdir -p "$tmp_dir"

tag=$1

yarn_uri=https://repo.yarnpkg.com/tags/$tag/$target

curl --fail --location --progress-bar --output "$archive" $yarn_uri ||
    error "Failed to download Yarn from \"$yarn_uri\""

unzip -q "$archive" -d "$tmp_dir"
rm "$tmp_dir"/yarn-bin
rm "$archive"

rm -rf "$install_dir"
mv "$tmp_dir" "$install_dir"

"$install_dir"/yarn switch postinstall
