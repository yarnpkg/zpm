#!/usr/bin/env bash

mkdir -p crates/greeter/src crates/hello/src

cat > package.json <<'JSON'
{
  "name": "e2e-rust-root",
  "private": true
}
JSON

cat > .yarnrc.yml <<'YAML'
unstableIslands:
  rust:
    workspaces:
      - "e2e-rust-root"
    linker: "cargo"
YAML

cat > rust-toolchain.toml <<'TOML'
[toolchain]
channel = "1.93.0"
TOML

cat > Cargo.toml <<'TOML'
[workspace]
resolver = "2"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2021"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
TOML

cat > crates/greeter/Cargo.toml <<'TOML'
[package]
name = "greeter"
version.workspace = true
edition.workspace = true

[dependencies]
serde.workspace = true

# Lockfiles cover every platform, so this one must be installed on Linux too
[target.'cfg(windows)'.dependencies]
windows-link = "0.2"
TOML

cat > crates/greeter/src/lib.rs <<'RUST'
use serde::Serialize;

#[derive(Serialize)]
pub struct Greeting {
    pub message: String,
}

pub fn greet(name: &str) -> Greeting {
    Greeting {message: format!("Hello {name} from crates installed by Yarn!")}
}
RUST

cat > crates/hello/Cargo.toml <<'TOML'
[package]
name = "hello"
version.workspace = true
edition.workspace = true

[dependencies]
greeter = { path = "../greeter" }
json = { package = "serde_json", version = "1" }

[dev-dependencies]
itoa = "1"
TOML

cat > crates/hello/src/main.rs <<'RUST'
fn main() {
    println!("{}", json::to_string(&greeter::greet("world")).unwrap());
}
RUST

yarn install

# Everything Cargo needs is in place without Cargo having been involved
grep -F 'replace-with = "yarn-vendor"' .cargo/config.toml

for crate in greeter hello serde serde_derive serde_json itoa windows-link; do
  grep -F "name = \"${crate}\"" Cargo.lock
done

# Crates are vendored, the workspace members aren't
ls -d .cargo/vendor/serde-1.* .cargo/vendor/serde_derive-1.* .cargo/vendor/serde_json-1.* .cargo/vendor/windows-link-0.2.*
[[ ! -e .cargo/vendor/greeter-0.1.0 ]]

# The resolution lives in Yarn's lockfile
grep -F 'serde@cargo:' yarn.lock
grep -F 'greeter@cargo:path+crates/greeter' yarn.lock

yarn install > "${TEMP_DIR}/second-install.txt"
grep -F "up-to-date" "${TEMP_DIR}/second-install.txt"

# Editing a Cargo.toml is noticed, and only adds what's missing
cp Cargo.lock "${TEMP_DIR}/Cargo.lock.before"
sed -i.bak 's/^itoa = "1"$/itoa = "1"\nfastrand = "2"/' crates/hello/Cargo.toml
yarn install
grep -F 'name = "fastrand"' Cargo.lock

function locked_versions() {
  awk '/^name = /{name = $3} /^version = /{print name, $3}' "${1}" | grep -v '^"fastrand"'
}

diff <(locked_versions "${TEMP_DIR}/Cargo.lock.before") <(locked_versions Cargo.lock)

# Building doesn't need anything from the machine either: Yarn provided
# the toolchain the project asks for. Cargo must accept what Yarn generated
# as-is (--locked), and never reach for the network (--offline).
CARGO_HOME="${TEMP_DIR}/cargo-home" yarn cargo run --offline --locked --quiet -p hello > "${TEMP_DIR}/output.txt"
grep -F '{"message":"Hello world from crates installed by Yarn!"}' "${TEMP_DIR}/output.txt"

yarn cargo --version | grep -F 'cargo 1.93.0'

# The commands editing dependencies work on the Cargo.toml of the crate
# they're run from
(cd crates/hello && yarn add cargo:itoa && yarn remove fastrand)
grep -F 'itoa = "1.' crates/hello/Cargo.toml
! grep -F 'fastrand' crates/hello/Cargo.toml Cargo.lock

yarn why itoa | grep -F 'hello@cargo:path+crates/hello'
