---
category: protocols
slug: protocol/cargo
title: Cargo Protocol
description: How Rust crates work in Yarn.
---

:::caution
Rust support is a prototype. It handles real-world workspaces (it was validated on Yarn's own, whose 557 crates Cargo then builds offline), but some Cargo features aren't covered yet; see the [limitations](#limitations).
:::

Yarn can install the crates a Rust project depends on: it resolves them, downloads them, and lays them out so that Cargo builds from them without touching the network. None of this requires a Rust toolchain; Cargo only gets involved once something is built.

```yaml
unstableIslands:
  rust:
    workspaces: [my-project]
    linker: cargo
```

```sh
yarn install                     # resolves, fetches, and vendors the crates
cargo build --offline --locked   # builds from what Yarn installed
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
| `cargo:git+https://example.com/repo.git?branch=main` | A crate from a git repository (`?branch=`, `?tag=`, `?rev=`, or nothing for the default branch). |
| `cargo:path+crates/core` | A crate of the project, by its path from the project root. |

## Resolution

Crates are resolved by an island, with Cargo's rules rather than npm's or PyPI's:

- A graph can hold several versions of a crate as long as they aren't semver-compatible: `1.2.0` and `1.9.0` are unified, `1.9.0` and `2.0.0` coexist.
- Features are unified: each crate gets the union of the features its dependents ask for, which decides which optional dependencies are part of the graph. Versions lacking a requested feature aren't candidates.
- Pre-releases are only selected by requirements mentioning one, and yanked versions only when they're already locked.
- The resolution covers every platform at once, with all the features of the workspace members enabled and their dev-dependencies included. That's what Cargo stores in `Cargo.lock`, and it keeps the lockfile valid whatever gets built afterwards.

The result is stored in `yarn.lock` along with a hash of the manifests it was computed from; as long as they don't change, installs don't contact the registry at all.

The first time an island is resolved, the versions of an existing `Cargo.lock` are preferred, so adopting Yarn doesn't upgrade anything. From then on `yarn.lock` is the source of truth, and `Cargo.lock` is derived from it.

## Registry

Crates are looked up in the [sparse index](https://doc.rust-lang.org/cargo/reference/registry-index.html#sparse-protocol) set by `cargoRegistryServer`, which defaults to crates.io:

```yaml
cargoRegistryServer: https://index.crates.io/
```

The index lists the dependencies, features, and checksum of every version, so resolving doesn't download any crate. Downloaded crates are checked against the checksum the index advertises, and stored as published (`.crate` files) in Yarn's cache.

Git dependencies follow the same rule as elsewhere: their repository must match one of the `approvedGitRepositories` patterns.

## What gets installed

For each workspace of the island, `yarn install` generates what `cargo vendor` and `cargo generate-lockfile` would:

| Path | Content |
| --- | --- |
| `.cargo/vendor/` | The unpacked crates, as a [directory source](https://doc.rust-lang.org/cargo/reference/source-replacement.html#directory-sources). Should be ignored by git. |
| `.cargo/config.toml` | A block replacing crates.io (and the git repositories the island depends on) with that directory. The rest of the file is left untouched. |
| `Cargo.lock` | The versions Yarn resolved, in Cargo's own format. When the island was resolved from an existing `Cargo.lock`, the file comes out byte for byte identical. |

Cargo, rust-analyzer, and the tools built on `cargo metadata` then work as usual, offline, and without downloading anything in `~/.cargo`.

Editing a `Cargo.toml` requires running `yarn install` again, like editing a `package.json` does: Cargo can't fetch new crates by itself anymore, as its sources are replaced.

## Limitations

- **Building:** Yarn doesn't build crates, and doesn't provision the Rust toolchain; `cargo` and `rustc` must be installed to build.
- **Patches:** `[patch]` and `[replace]` tables are reported as unsupported.
- **Registries:** crates can only come from the registry set by `cargoRegistryServer`, without authentication. Dependencies on another registry (`registry = "..."`) are reported as unsupported.
- **Git dependencies:** submodules aren't fetched.
- **Rust versions:** the resolution doesn't take the `rust-version` of the crates into account (Cargo's `resolver = "3"` prefers the versions compatible with the toolchain).
- **Commands:** `yarn add`, `yarn up` and the other commands editing manifests don't know about `Cargo.toml` files.
