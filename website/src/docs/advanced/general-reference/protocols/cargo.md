---
category: protocols
slug: protocol/cargo
title: Cargo Protocol
description: How Rust crates work in Yarn.
---

:::caution
Rust support is experimental. It handles real-world workspaces (it was validated on Yarn's own, whose 557 crates Cargo then builds offline), and the lockfiles it generates are checked against Cargo's in its test suite; see the [limitations](#limitations) for what isn't covered.
:::

Yarn can install the crates a Rust project depends on: it resolves them, downloads them, and lays them out so that Cargo builds from them without touching the network. It doesn't need anything from the machine to do so, and provides the Rust toolchain itself when something has to be built.

```yaml
unstableIslands:
  rust:
    workspaces: [my-project]
    linker: cargo
```

```sh
yarn install                 # resolves, fetches, and vendors the crates
yarn cargo build --release   # builds them with the project's toolchain
```

## Manifests

Unlike the other protocols, crates aren't listed in `package.json`. An island using the `cargo` linker reads the `Cargo.toml` found next to the `package.json` of each of its workspaces, which is either a crate or the root of a Cargo workspace. Cargo needs those files to build anyway, and they describe things a `package.json` can't (features, build and target-specific dependencies), so they stay the source of truth.

A few consequences:

- The members of a Cargo workspace don't have to be Yarn workspaces; they're found through `workspace.members`, like Cargo does.
- The `dependencies` of the workspace's `package.json` keep being installed as usual. A Rust repository with a few JavaScript tools at its root can list that root in a cargo island.
- Workspace inheritance (`version.workspace = true`, `serde = { workspace = true }`), renamed dependencies (`package = "..."`), target-specific tables, and build and dev-dependencies are supported.

The `cargo:` protocol is how those dependencies are recorded in the lockfile:

| Range | Meaning |
| --- | --- |
| `cargo:^1.0.207` | A crate from the registry, with [Cargo's requirement syntax](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html) (`1.2` means `^1.2`, comparators are separated by commas). |
| `cargo:serde_json@^1` | A renamed dependency; the name before the `@` is the crate's. |
| `cargo:^1#https://crates.example.org/index/` | A crate from another registry, by the URL of its index. |
| `cargo:git+https://example.com/repo.git?branch=main` | A crate from a git repository (`?branch=`, `?tag=`, `?rev=`, or nothing for the default branch). |
| `cargo:path+crates/core` | A crate of the project, by its path from the project root. |

## Resolution

Crates are resolved by an island, with Cargo's rules rather than npm's or PyPI's:

- A graph can hold several versions of a crate as long as they aren't semver-compatible: `1.2.0` and `1.9.0` are unified, `1.9.0` and `2.0.0` coexist.
- Features are unified: each crate gets the union of the features its dependents ask for, which decides which optional dependencies are part of the graph. Versions lacking a requested feature aren't candidates.
- Pre-releases are only selected by requirements mentioning one, and yanked versions only when they're already locked.
- The resolution covers every platform at once, with all the features of the workspace members enabled and their dev-dependencies included. That's what Cargo stores in `Cargo.lock`, and it keeps the lockfile valid whatever gets built afterwards.
- `[patch]` tables add candidates (a fork on the disk or in a git repository) that are preferred to the published versions whenever they satisfy the requirement; `[replace]` tables are supported too.
- With the third version of Cargo's resolver (`resolver = "3"`, the default of the 2024 edition), the versions whose `rust-version` the project supports are preferred to newer ones. The project's Rust version is the `rust-version` of its crates or, without any, the version its `rust-toolchain.toml` pins.

The result is stored in `yarn.lock` along with a hash of the manifests it was computed from; as long as they don't change, installs don't contact the registry at all.

The first time an island is resolved, the versions of an existing `Cargo.lock` are preferred, so adopting Yarn doesn't upgrade anything. From then on `yarn.lock` is the source of truth, and `Cargo.lock` is derived from it.

## Registries

Crates are looked up in the [sparse index](https://doc.rust-lang.org/cargo/reference/registry-index.html#sparse-protocol) set by `cargoRegistryServer`, which defaults to crates.io:

```yaml
cargoRegistryServer: https://index.crates.io/
```

The index lists the dependencies, features, and checksum of every version, so resolving doesn't download any crate. Downloaded crates are checked against the checksum the index advertises, and stored as published (`.crate` files) in Yarn's cache.

### Other registries

Dependencies naming a registry (`private = { version = "1", registry = "internal" }`) are resolved from it. Registries are read from the `[registries]` table of the project's `.cargo/config.toml`, and from the `cargoRegistries` setting, which is also where their credentials go:

```yaml
cargoRegistries:
  internal:
    cargoRegistryServer: sparse+https://crates.example.org/index/
    cargoAuthToken: ${CRATES_TOKEN:-}
```

A registry only defined in `cargoRegistries` is declared to Cargo in the block Yarn manages in `.cargo/config.toml`, so it doesn't have to be written twice.

### Authentication

`cargoAuthToken` (at the top level for the default registry, or in a `cargoRegistries` entry) is sent as the `Authorization` header, like Cargo does. It's sent to the index, and to the host serving the crates only when it's the same one or when the registry says downloads need it (`auth-required`). Values support environment variable interpolation.

### Git dependencies

Crates can come from git repositories (`git = "..."`, with `branch`, `tag` or `rev`), submodules included. As everywhere else in Yarn, the repositories must match one of the `approvedGitRepositories` patterns. The commit a branch points to is locked, and only moves when asked to (`yarn up -R <crate>`).

## What gets installed

For each workspace of the island, `yarn install` generates what `cargo vendor` and `cargo generate-lockfile` would:

| Path | Content |
| --- | --- |
| `.cargo/vendor/` | The unpacked crates, as a [directory source](https://doc.rust-lang.org/cargo/reference/source-replacement.html#directory-sources). Should be ignored by git. |
| `.cargo/config.toml` | A block replacing crates.io (and the git repositories the island depends on) with that directory. The rest of the file is left untouched. |
| `Cargo.lock` | The versions Yarn resolved, in Cargo's own format. When the island was resolved from an existing `Cargo.lock`, the file comes out byte for byte identical. |

Cargo, rust-analyzer, and the tools built on `cargo metadata` then work as usual, offline, and without downloading anything in `~/.cargo`.

Editing a `Cargo.toml` by hand requires running `yarn install` again, like editing a `package.json` does: Cargo can't fetch new crates by itself anymore, as its sources are replaced.

## Commands

The commands managing dependencies work on `Cargo.toml` files for everything using the `cargo:` protocol, keeping their formatting and comments:

```sh
cd crates/my-crate

yarn add cargo:serde            # latest release, written like Cargo users do (serde = "1.0.229")
yarn add -D cargo:rstest@0.26   # [dev-dependencies]
yarn add -O cargo:zstd          # optional = true
yarn add json@cargo:serde_json@1                        # renamed (package = "serde_json")
yarn add fork@cargo:git+https://example.com/fork.git    # from a repository

yarn up serde 'tokio*'          # bumps the requirements, in every Cargo.toml of the project
yarn up -R serde                # resolves again without touching the manifests
yarn remove zstd
yarn why serde
```

`yarn add` and `yarn remove` apply to the crate the current directory belongs to. Dependencies inherited from the workspace (`serde = { workspace = true }`) are changed where they're defined, in `[workspace.dependencies]`.

## Toolchain

Each workspace of a cargo island gets the Rust toolchain it asks for, the same way rustup would pick it:

- from its `rust-toolchain.toml` (or the closest one in a parent folder), including the `components` and `targets` it lists;
- otherwise from the `rustToolchain` setting, which defaults to `stable`.

```toml
[toolchain]
channel = "1.93.0"
components = ["clippy", "rustfmt"]
targets = ["wasm32-unknown-unknown"]
```

With `rustPreference: managed` (default), Yarn uses a toolchain from its global folder, then a matching one from the `PATH`, then downloads it from `rustDownloadMirror` (unless `rustDownloads: false`); with `rustPreference: system`, the `PATH` comes first. Downloads are the official archives of each component, checked against the checksums of the channel's manifest, and shared by every project on the machine.

`yarn cargo` runs Cargo from that toolchain, and the scripts of the workspace have it in their `PATH`:

```sh
yarn cargo test -p my-crate
yarn cargo clippy
```

Building native code still takes what Rust itself requires from the system: a C linker (`cc`), like with rustup.

## Limitations

- **Registries:** only sparse indexes are supported, not the git-based ones (crates.io is served through its sparse index).
- **Platforms:** the vendor directory holds the crates of every platform, as Cargo requires from directory sources, which makes it larger than what a single platform builds.
- **`yarn add`:** features can't be set from the command line yet; edit the entry in the `Cargo.toml`, then run `yarn install`.
- **Toolchains:** managed toolchains are available for the platforms Yarn itself supports (Linux glibc and musl, macOS, Windows x64).
