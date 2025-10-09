#!/usr/bin/env bash
set -euo pipefail

# Reset
color_off=''

# Regular Colors
color_error=''
color_yarn=''
color_url=''

truecolor() {
  echo -e "\x1b[38;2;${1};${2};${3}m"
}

if [[ -t 1 ]]; then
  # Reset
  color_off='\033[0m' # Text Reset

  # Regular Colors
  color_error=$(truecolor 200 100 100)
  color_yarn=$(truecolor 100 180 215)
  color_url=$(truecolor 215 130 215)
fi

colorize() {
  echo -e "$1$2${color_off}"
}

error() {
  echo "$(colorize "$color_error" "Error:") $1" >&2
  exit 1
}

command -v unzip >/dev/null ||
  error 'The unzip command is required to install Yarn'

bin_dir=
install_channel=stable
positional_args=()

while [[ $# -gt 0 ]]; do
  case $1 in
    --canary)
      install_channel=canary
      shift
      ;;

    --bin-dir)
      bin_dir="$2"
      shift
      shift
      ;;

    -*|--*)
      echo "Unknown option $1"
      exit 1
      ;;

    *)
      positional_args+=("$1")
      shift
      ;;
  esac
done

if [[ ${#positional_args[@]} -gt 1 ]]; then
    error 'Too many arguments, only one representing a specific tag to install is allowed. (e.g. "6.0.0")'
fi

platform=$(uname -ms)

case $platform in
'Darwin arm64')
  target=aarch64-apple-darwin
  ;;
'Linux aarch64' | 'Linux arm64')
  target=aarch64-unknown-linux-musl
  ;;
'Linux x86_64' | *)
  target=x86_64-unknown-linux-musl
  ;;
esac

install_dir=$HOME/.yarn/switch/bin
tmp_dir=$install_dir.tmp
archive=$tmp_dir/yarn.zip

rm -rf "$tmp_dir"
mkdir -p "$tmp_dir"

yarn_version=${positional_args[0]:-$(curl --fail --location -s https://repo.yarnpkg.com/channels/default/$install_channel)}
yarn_uri=https://repo.yarnpkg.com/releases/$yarn_version/$target

echo "This script will install or update $(colorize "$color_yarn" "Yarn Switch"), a utility that lets you lock Yarn versions in your projects."
echo "For more information, please take a look at our documentation at $(colorize "$color_url" "https://yarnpkg.com")"
echo

curl --fail --location --progress-bar --output "$archive" $yarn_uri ||
    error "Failed to download Yarn from $(colorize "$color_url" "$yarn_uri")"

unzip -q "$archive" -d "$tmp_dir"
rm "$tmp_dir"/yarn-bin
rm "$archive"

if [[ -n "$bin_dir" ]]; then
  mv -f "$tmp_dir"/yarn "$bin_dir"/
else
  rm -rf "$install_dir"
  mv "$tmp_dir" "$install_dir"

  echo

  "$install_dir"/yarn switch postinstall -H "$HOME"
fi
