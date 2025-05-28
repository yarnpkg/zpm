#!/usr/bin/env bash
set -euo pipefail

# Reset
Color_Off=''

# Regular Colors
Error=''
Yarn=''
Url=''

truecolor() {
    echo -e "\x1b[38;2;${1};${2};${3}m"
}

if [[ -t 1 ]]; then
    # Reset
    Color_Off='\033[0m' # Text Reset

    # Regular Colors
    Error=$(truecolor 255 0 0)
    Yarn=$(truecolor 100 180 215)
    Url=$(truecolor 215 95 215)
fi

colorize() {
    echo -e "$1$2${Color_Off}"
}

error() {
    echo "$(colorize $Error "Error:") $1" >&2
    exit 1
}

command -v unzip >/dev/null ||
    error 'The unzip command is required to install Yarn'

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

echo "This script will install or update $(colorize $Yarn "Yarn Switch"), a utility that lets you lock Yarn versions in your projects."
echo "For more information, please take a look at our documentation at $(colorize $Url "https://yarnpkg.com")"
echo

curl --fail --location --progress-bar --output "$archive" $yarn_uri ||
    error "Failed to download Yarn from $(colorize $Url "$yarn_uri")"

unzip -q "$archive" -d "$tmp_dir"
rm "$tmp_dir"/yarn-bin
rm "$archive"

rm -rf "$install_dir"
mv "$tmp_dir" "$install_dir"

echo

"$install_dir"/yarn switch postinstall
