# Rust support

This document reviews how Rust dependencies can be supported the way Python dependencies are, and describes the prototype that came out of it. The user-facing side is documented in the [`cargo:` protocol](../../website/src/docs/advanced/general-reference/protocols/cargo.md) page.

In short: an island using the `cargo` linker reads the `Cargo.toml` of its workspaces, resolves the crates with Cargo's own rules, stores the result in `yarn.lock`, downloads the crates into Yarn's cache, and generates what Cargo needs to build from them offline (a vendor directory, a `Cargo.lock`, a source replacement block). Nothing in there runs Cargo.

## What "like Python" means

Python support is made of the pieces below; each has a Rust counterpart.

| Piece | Python | Rust | Prototype |
| --- | --- | --- | --- |
| Protocol | `pypi:` ranges, PEP 440 | `cargo:` ranges, Cargo's semver flavor | Done |
| Manifest | `package.json` (imported from `pyproject.toml`) | `Cargo.toml`, read in place | Done |
| Registry client | Simple API (PEP 503 / 691 / 658) | Sparse index | Done, without authentication |
| Resolution | Island + PubGrub, one version per package | Island + PubGrub, one version per semver bucket, features | Done |
| Lockfile | `yarn.lock`, seeded from `uv.lock` | `yarn.lock`, seeded from `Cargo.lock` | Done |
| Fetching | Wheels and sdists in the cache | `.crate` files in the cache, git checkouts | Done |
| Linking | `.venv` per workspace | Vendor directory, `Cargo.lock`, `.cargo/config.toml` per workspace | Done |
| Toolchain | python-build-standalone download | `rust-toolchain.toml` + static.rust-lang.org | Not done |
| Builds | sdists built through PEP 517 | Everything is source; Cargo builds | Left to Cargo |
| Running | `yarn python`, scripts run in the venv | `yarn cargo`, scripts with the toolchain in the `PATH` | Not done |
| Import | `yarn import uv` | Not needed: nothing has to be converted | - |

## Where Rust differs

Four differences shaped the design; the rest follows the Python implementation closely.

### The manifest has to stay `Cargo.toml`

Python workspaces declare their dependencies in `package.json` because nothing else reads them once the venv exists. Rust is different: whatever builds the crates needs the dependency tables, with details a `package.json` has no place for (features, `default-features`, renames, build and dev kinds, `cfg()` targets). Moving them would mean either keeping two manifests in sync, or generating `Cargo.toml` files.

So a cargo island reads the `Cargo.toml` next to the `package.json` of its workspaces. Two things follow:

- The island's workspaces aren't taken out of the regular resolution like those of the other islands are: their `package.json` dependencies still go through it. That's what lets the root of a repository like this one (JavaScript tooling in `package.json`, a Cargo workspace in `Cargo.toml`) be listed in a cargo island.
- Cargo workspace members don't have to be Yarn workspaces. They're discovered from `workspace.members` and recorded as `cargo:path+<path>` packages.

`cargo:` ranges in a `package.json` are rejected for now (they can't be resolved one at a time, and nothing would build them). They would become meaningful if Yarn ever built crates itself, see [further down](#going-further).

### Resolution follows other rules

The island machinery (PubGrub, locked versions preferred, input hash to skip the solver) is reused, but with a dedicated provider ([`cargo/solver.rs`](../../packages/zpm/src/cargo/solver.rs)) rather than new cases in the existing one, as almost none of the rules are shared:

- **Buckets.** Several versions of a crate can coexist, one per semver-compatible range (`1.x`, `0.4.x`, `0.0.7`). The solver's packages are (crate, bucket) pairs.
- **Features.** A crate is built once with the union of the features its dependents ask for, and features enable optional dependencies. Each requested feature is a proxy package pinned to its crate's version, whose dependencies are what the feature enables - the same trick as PyPI extras, generalized to `feature`, `dep:name`, `name/feature` and `name?/feature`. A version lacking a requested feature has no proxy, so the solver falls back on one that has it, like Cargo does.
- **Wide requirements.** `>=0.48, <=0.61` can be satisfied from several buckets. Those go through one more proxy with one version per bucket.
- **One resolution for every platform.** There's no equivalent of markers being evaluated per target: like `Cargo.lock`, the resolution includes the dependencies of every `cfg()`, every feature of the workspace members, and their dev-dependencies. It deliberately over-approximates what gets built (weak dependency features do pull the dependency in, as in Cargo's lockfiles).
- **Versions.** Requirements use Cargo's syntax, pre-releases only match requirements naming one, yanked versions are only eligible when locked, and build metadata is part of a version's identity (`0.11.1+wasi-snapshot-preview1`). `zpm_semver` models npm's flavor, so those live in [`zpm-primitives/src/cargo.rs`](../../packages/zpm-primitives/src/cargo.rs).

Version sets are built from the candidates the index lists rather than from interval arithmetic on requirements, which keeps the matching exactly Cargo's (pre-release rules included) at no cost.

### Installing isn't building

Crates are only published as sources, so there's no equivalent of unpacking wheels into a venv: "installed" means "available to the compiler". Building stays Cargo's job, the same way running stays Python's. Not requiring Cargo therefore applies to everything up to that point: resolving, fetching and laying crates out.

### The consumer is Cargo

The link step has to produce something Cargo accepts as-is. Three artifacts do that, all documented and stable:

- a [directory source](https://doc.rust-lang.org/cargo/reference/source-replacement.html#directory-sources) (`.cargo/vendor`), what `cargo vendor` produces;
- a managed block in `.cargo/config.toml` replacing crates.io and each git source with it;
- a `Cargo.lock` rendered from `yarn.lock`.

`Cargo.lock` becomes a derived file, like `.pnp.cjs`: `yarn.lock` is what changes when dependencies do. Since Cargo's sources are replaced, it can't silently diverge - a `Cargo.toml` edited without running `yarn install` makes Cargo fail to find the new crate rather than fetch it.

Alternatives that were considered for the layout:

| Option | Why not (yet) |
| --- | --- |
| Local registry source (`.crate` files + index files, unpacked lazily by Cargo) | Much cheaper to link and to store, as nothing is unpacked by Yarn. But git crates can't be served from it (Cargo refuses a replacement that adds checksums), so it takes two kinds of sources. The natural next step. |
| Populating `$CARGO_HOME/registry` | No configuration needed, but the layout is internal to Cargo, and its folder names changed across versions. |
| Passing `--config` through a `yarn cargo` wrapper instead of writing `.cargo/config.toml` | Tools calling Cargo themselves (rust-analyzer) would then use the network sources. |

## Implementation

| Stage | Where | Notes |
| --- | --- | --- |
| Primitives | [`zpm-primitives/src/cargo.rs`](../../packages/zpm-primitives/src/cargo.rs), `Range::{Cargo, CargoGit, CargoPath}`, `Reference::{CargoRegistry, CargoGit, CargoPath}` | Git sources are written like Cargo's source ids, which makes rendering `Cargo.lock` trivial. |
| Manifests | [`cargo/manifest.rs`](../../packages/zpm/src/cargo/manifest.rs) | Members, inheritance, target tables, implicit features. Also writes the standalone manifest of git crates taken out of their workspace (what `cargo package` does). |
| Index | [`cargo/index.rs`](../../packages/zpm/src/cargo/index.rs) | One file per crate, persisted in the global folder; not queried when the listing on disk has the locked version. |
| Git | [`cargo/git.rs`](../../packages/zpm/src/cargo/git.rs) | One checkout per commit in the global folder; honors `approvedGitRepositories`. |
| Solver | [`cargo/solver.rs`](../../packages/zpm/src/cargo/solver.rs) | Behind a `CrateCatalog` trait, so that the rules are tested without a registry. |
| Island | [`cargo/island.rs`](../../packages/zpm/src/cargo/island.rs) | Discovery of the local crates, input hash, seeding, conversion to `Resolution`s. |
| Fetching | [`fetchers/cargo.rs`](../../packages/zpm/src/fetchers/cargo.rs) | Checks the sha256 the index advertises. |
| Linking | [`linker/cargo.rs`](../../packages/zpm/src/linker/cargo.rs), [`cargo/lockfile.rs`](../../packages/zpm/src/cargo/lockfile.rs) | Vendoring is incremental and atomic per crate. |

In `yarn.lock`, crates look like any other island package:

```json
"serde@cargo:1, serde@cargo:^1.0.220": {
  "checksum": "...",
  "resolution": {
    "resolution": "serde@cargo:1.0.229",
    "version": "1.0.229",
    "dependencies": {"serde_core": "cargo:=1.0.229", "serde_derive": "cargo:^1"}
  }
}
```

Only what identifies an edge is stored: the dependencies that ended up enabled (`serde_derive` is optional, and there because a dependent asked for the `derive` feature), and what they resolved to. Features, kinds and targets aren't, as they can be read back from the manifests (the index, or the crate itself) whenever something needs them. A renamed dependency keeps its local name as key (`divan@cargo:codspeed-divan-compat@4.2.0`), while locators always carry the crate's name, so a crate is fetched once however it's referred to.

Cargo manifests aren't tracked by the checks that let `yarn install` return early, so the linker stores a digest of the manifests next to the vendor directory, and `Project::is_install_up_to_date` compares it.

## Validation

All of this was run against this repository's own Cargo workspace (14 members, 537 registry crates, 6 crates from 4 git repositories, 4 coexisting versions of `windows-sys`, renamed and target-specific dependencies, 63 crates using weak dependency features):

- Resolving with the existing `Cargo.lock` as seed regenerates it **byte for byte**. The versions come from the seed, but the set of packages and every dependency list come from the solver, so this checks the feature and optional dependency logic against Cargo's on the whole graph.
- `cargo metadata --offline --locked` and `cargo build --offline --locked` succeed with an empty `CARGO_HOME`, and the resulting binary runs. `--locked` makes Cargo fail if it would change the lockfile in any way.
- Resolving from scratch (no `Cargo.lock`) gives 539 packages, also accepted by `cargo metadata --offline --locked`. Compared to `cargo generate-lockfile` on the same day, 537 are identical; the two others (`crypto-common` 0.1.7 + `generic-array` 0.14.7, where Cargo picked 0.1.6 + 0.14.9) are another valid answer to a conflict whose outcome depends on the order decisions are made in.
- Timings on a 4-core container: 5.5s for a cold install (69 MiB downloaded), 9s when resolving from scratch, 2.5s to rebuild a removed vendor directory, 1.7s after adding a dependency, 20ms when there's nothing to do.

Automated coverage: unit tests for the requirement semantics, the manifest reader, the index parser, the solver rules and the lockfile writer; integration tests (`features/cargo.test.ts`) on registry-less projects; an e2e test (`tests/e2e/rust.sh`) against crates.io, which also builds with Cargo when it's available.

## Not covered yet

Roughly by order of importance:

1. **`[patch]` and `[replace]`.** Reported as unsupported rather than ignored. They map well to `resolutions`.
2. **Other registries and authentication.** A single sparse index, no credentials. `packageRules` / `sourceRules` with an `ecosystemFilter: cargo` would follow what PyPI does; dependencies with `registry = "..."` need the registry to be part of the package identity.
3. **A registry mock for the integration tests**, so that the registry path is tested hermetically (the PyPI tests have one).
4. **Disk usage.** The vendor directory holds every crate of every platform (541 MB here), as directory sources require. Unpacking once in the global folder and hardlinking (like the venv store), or switching to a local registry source, would both help.
5. **Age gate.** The index now reports publication times (`pubtime`), so a `cargoMinimalAgeGate` is a small addition.
6. **Rust version awareness.** `resolver = "3"` prefers the versions whose `rust-version` the toolchain satisfies; the index has the field.
7. **`links`.** Two crates linking the same native library are detected after the fact, as an error, instead of being a constraint the solver can work around.
8. **Git submodules**, and storing git crates in the cache (they're used from the global folder, so an offline mirror doesn't cover them).
9. **Commands.** `yarn add`, `yarn up`, `yarn why`, `yarn info` and `yarn dedupe` know nothing about `Cargo.toml`.
10. **Lockfile checksums** are those of Yarn's cache; `Cargo.lock`'s sha256 are recomputed from the archives at link time. Storing them would make linking a little cheaper.

## Going further

**Provisioning the toolchain.** This is the direct equivalent of the managed Python interpreters. `rust-toolchain.toml` says which channel a project wants, and `https://static.rust-lang.org/dist/channel-rust-<channel>.toml` lists the archives of each component per target, with their sha256 (both `.tar.gz` and `.tar.xz`). Downloading `rustc`, `rust-std` and `cargo` into the global folder, then exposing them through a `yarn cargo` command and the scripts' `PATH`, would make `yarn install && yarn cargo build` work on a machine that has neither rustup nor Cargo. It's self-contained and doesn't depend on anything else in this document.

**Building without Cargo.** Driving `rustc` directly (what Buck and Bazel do) would make Yarn the build tool: `package.json` could then hold the dependencies, and `cargo:` ranges would work anywhere. It's a different order of magnitude, though. It takes the per-target feature resolution Cargo runs at build time (the lockfile's is an over-approximation), `cfg()` evaluation, build scripts and their protocol (`cargo:rustc-cfg`, `links` metadata, `OUT_DIR`), proc-macros built for the host, profiles, and fingerprinting for incremental builds - and rust-analyzer, clippy and `cargo test` would still expect a Cargo project. The resolution data needed for it is all there, but provisioning Cargo is a much better trade.

## Open questions

- **Where artifacts go.** The prototype writes inside `.cargo/` (a block in `config.toml`, and `vendor/`, which has to be git-ignored). Is editing `.cargo/config.toml` acceptable, or should it be opt-in?
- **Should `Cargo.lock` be committed?** It's derived, so it could be ignored like `.pnp.cjs` often is; keeping it makes the repository usable without Yarn, and diffs readable to Rust developers.
- **Should crates be Yarn workspaces?** They aren't required to be, which keeps adoption to three lines of configuration. Making them workspaces would bring `yarn workspaces foreach`, constraints and the release workflow to crates, at the cost of a `package.json` per crate.
- **Edges between ecosystems.** A JavaScript workspace wrapping a crate (napi, wasm) can't express that dependency today; islands are separate graphs.
