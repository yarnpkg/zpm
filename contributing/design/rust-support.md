# Rust support

This document reviews how Rust dependencies can be supported the way Python dependencies are, and describes the prototype that came out of it. The user-facing side is documented in the [`cargo:` protocol](../../website/src/docs/advanced/general-reference/protocols/cargo.md) page.

In short: an island using the `cargo` linker reads the `Cargo.toml` of its workspaces, resolves the crates with Cargo's own rules, stores the result in `yarn.lock`, downloads the crates into Yarn's cache, and generates what Cargo needs to build from them offline (a vendor directory, a `Cargo.lock`, a source replacement block). Nothing in there runs Cargo, and nothing has to be installed beforehand: the toolchain used to build is provisioned by Yarn too.

## What "like Python" means

Python support is made of the pieces below; each has a Rust counterpart.

| Piece | Python | Rust | Prototype |
| --- | --- | --- | --- |
| Protocol | `pypi:` ranges, PEP 440 | `cargo:` ranges, Cargo's semver flavor | Done |
| Manifest | `package.json` (imported from `pyproject.toml`) | `Cargo.toml`, read in place | Done |
| Registry client | Simple API (PEP 503 / 691 / 658) | Sparse index, several registries, tokens | Done |
| Resolution | Island + PubGrub, one version per package | Island + PubGrub, one version per semver bucket, features | Done |
| Lockfile | `yarn.lock`, seeded from `uv.lock` | `yarn.lock`, seeded from `Cargo.lock` | Done |
| Fetching | Wheels and sdists in the cache | `.crate` files in the cache, git checkouts | Done |
| Linking | `.venv` per workspace | Vendor directory, `Cargo.lock`, `.cargo/config.toml` per workspace | Done |
| Toolchain | python-build-standalone download | `rust-toolchain.toml` + static.rust-lang.org | Done |
| Builds | sdists built through PEP 517 (with pip) | Everything is source; built by the provisioned Cargo | Done |
| Running | `yarn python`, scripts run in the venv | `yarn cargo`, scripts with the toolchain in the `PATH` | Done |
| Commands | `yarn add` & co edit `package.json` | `yarn add` & co edit `Cargo.toml` | Done, through a generic layer |
| Import | `yarn import uv` | Not needed: nothing has to be converted | - |

## Where Rust differs

Four differences shaped the design; the rest follows the Python implementation closely.

### The manifest has to stay `Cargo.toml`

Python workspaces declare their dependencies in `package.json` because nothing else reads them once the venv exists. Rust is different: whatever builds the crates needs the dependency tables, with details a `package.json` has no place for (features, `default-features`, renames, build and dev kinds, `cfg()` targets). Moving them would mean either keeping two manifests in sync, or generating `Cargo.toml` files.

So a cargo island reads the `Cargo.toml` next to the `package.json` of its workspaces. Two things follow:

- The island's workspaces aren't taken out of the regular resolution like those of the other islands are: their `package.json` dependencies still go through it. That's what lets the root of a repository like this one (JavaScript tooling in `package.json`, a Cargo workspace in `Cargo.toml`) be listed in a cargo island.
- Cargo workspace members don't have to be Yarn workspaces. They're discovered from `workspace.members` and recorded as `cargo:path+<path>` packages.

`cargo:` ranges in a `package.json` are rejected (they can't be resolved one at a time, and nothing would build them). They would become meaningful if Yarn ever drove the compiler itself, see [further down](#building-without-cargo).

### Resolution follows other rules

The island machinery (PubGrub, locked versions preferred, input hash to skip the solver) is reused, but with a dedicated provider ([`cargo/solver.rs`](../../packages/zpm/src/cargo/solver.rs)) rather than new cases in the existing one, as almost none of the rules are shared:

- **Buckets.** Several versions of a crate can coexist, one per semver-compatible range (`1.x`, `0.4.x`, `0.0.7`). The solver's packages are (crate, bucket) pairs.
- **Features.** A crate is built once with the union of the features its dependents ask for, and features enable optional dependencies. Each requested feature is a proxy package pinned to its crate's version, whose dependencies are what the feature enables - the same trick as PyPI extras, generalized to `feature`, `dep:name`, `name/feature` and `name?/feature`. A version lacking a requested feature has no proxy, so the solver falls back on one that has it, like Cargo does.
- **Wide requirements.** `>=0.48, <=0.61` can be satisfied from several buckets. Those go through one more proxy with one version per bucket.
- **One resolution for every platform.** There's no equivalent of markers being evaluated per target: like `Cargo.lock`, the resolution includes the dependencies of every `cfg()`, every feature of the workspace members, and their dev-dependencies. It deliberately over-approximates what gets built (weak dependency features do pull the dependency in, as in Cargo's lockfiles).
- **Versions.** Requirements use Cargo's syntax, pre-releases only match requirements naming one, yanked versions are only eligible when locked, and build metadata is part of a version's identity (`0.11.1+wasi-snapshot-preview1`). `zpm_semver` models npm's flavor, so those live in [`zpm-primitives/src/cargo.rs`](../../packages/zpm-primitives/src/cargo.rs).
- **Preferences.** Candidates are ranked like Cargo ranks them: what the lockfile holds and the `[patch]` entries first, then (with resolver 3) what supports the workspace's Rust version, then the highest version. The ranking applies across buckets too, which matters for wide requirements.
- **Patches and replacements.** A `[patch]` entry is one more candidate for its crate, hiding the published version it shares its version with; a patch nothing ends up using stays in the island as a package without dependents, which is what `Cargo.lock` calls `[[patch.unused]]`. A `[replace]` entry keeps the registry crate in the graph (dependents still point to it) with the dependencies and features of its replacement, which it's recorded as depending on.
- **Registries.** A registry is identified by the URL of its index and is part of a crate's identity (`cargo:1.2.3#<index>`); the dependencies of a crate come from its own registry unless the index says otherwise.

Version sets are built from the candidates the index lists rather than from interval arithmetic on requirements, which keeps the matching exactly Cargo's (pre-release rules included) at no cost.

### Installing isn't building

Crates are only published as sources, so there's no equivalent of unpacking wheels into a venv: "installed" means "available to the compiler". Resolving, fetching and laying crates out are done by Yarn; compiling is done by Cargo, the same way Python source distributions are built by pip - a Cargo that Yarn provides, though, like it provides the Python interpreter.

The toolchain is read from `rust-toolchain.toml` (what rustup reads), assembled from the official distribution (the channel manifest lists an archive and its sha256 for each component and target), and kept in the global folder. `yarn cargo` runs it, and the scripts of the island's workspaces get it in their `PATH`. See [`cargo/toolchain.rs`](../../packages/zpm/src/cargo/toolchain.rs).

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
| Passing `--config` through `yarn cargo` instead of writing `.cargo/config.toml` | Tools calling Cargo themselves (rust-analyzer) would then use the network sources. Possible now that `yarn cargo` exists, as an opt-in. |

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
| Toolchain | [`cargo/toolchain.rs`](../../packages/zpm/src/cargo/toolchain.rs), [`commands/cargo.rs`](../../packages/zpm/src/commands/cargo.rs) | Managed toolchains are completed in place when a workspace asks for more components or targets. |
| Commands | [`manifest/dependencies.rs`](../../packages/zpm/src/manifest/dependencies.rs), [`cargo/editor.rs`](../../packages/zpm/src/cargo/editor.rs) | See below. |

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

Cargo islands are part of the resolution tree like the others, which is what `yarn why` reads.

## Manifests of other ecosystems

`yarn add`, `yarn up` and `yarn remove` used to edit `package.json` directly. They now go through [`manifest/dependencies.rs`](../../packages/zpm/src/manifest/dependencies.rs), which is where "dependencies aren't all declared in `package.json`" is dealt with, once for every command:

- `DependencyManifest` is a file declaring dependencies: it says which ranges are its own (`accepts`), lists what it declares, and knows how to `set`, `update` and `remove` an entry. `PackageManifest` implements it for `package.json`, `CargoManifestEditor` for `Cargo.toml`.
- `ManifestSet` is what a workspace's dependencies are declared in: its `package.json`, plus the manifests of the ecosystems it's part of. Commands ask it for the manifest a range belongs to, or iterate over all of them.
- `upgrade_descriptor` is the ecosystem's notion of "latest" (`yarn up`), and `LooseDescriptor` has a form per ecosystem on the command line (`cargo:serde`, like `jsr:@std/path`).

A range is routed by its protocol: `cargo:` ranges only ever go to a `Cargo.toml`, everything else to `package.json`. Names aren't unique across ecosystems (`url`, `uuid`, `colored` exist on both npm and crates.io), so `yarn up colored` upgrades each declaration within its own registry.

Supporting `pyproject.toml` is a matter of implementing `DependencyManifest` for it (accepting `pypi:` ranges for the workspaces that opt in) and returning it from `companion_manifests`; the commands don't change.

What's specific to Cargo stays in its editor: the crate the current directory designates rather than the Yarn workspace, `[workspace.dependencies]` for inherited entries, `optional = true` for `-O`, comments and key order preserved through `toml_edit`.

## Validation

Cargo is the reference for everything above, so the tests use it as a judge wherever they can: `cargo metadata --offline --locked` fails if the lockfile Yarn generated needs any change, or if anything is missing from the vendor directory.

Against this repository's own Cargo workspace (14 members, 537 registry crates, 6 crates from 4 git repositories, 4 coexisting versions of `windows-sys`, renamed and target-specific dependencies, 63 crates using weak dependency features):

- Resolving with the existing `Cargo.lock` as seed regenerates it **byte for byte**. The versions come from the seed, but the set of packages and every dependency list come from the solver, so this checks the feature and optional dependency logic against Cargo's on the whole graph.
- `cargo build --offline --locked` succeeds with an empty `CARGO_HOME`, and the resulting binary runs.
- Resolving from scratch (no `Cargo.lock`) gives 539 packages, also accepted by Cargo. Compared to `cargo generate-lockfile` on the same day, 537 are identical; the two others (`crypto-common` 0.1.7 + `generic-array` 0.14.7, where Cargo picked 0.1.6 + 0.14.9) are another valid answer to a conflict whose outcome depends on the order decisions are made in.
- Timings on a 4-core container: 5.5s for a cold install (69 MiB downloaded), 9s when resolving from scratch, 2.5s to rebuild a removed vendor directory, 1.7s after adding a dependency, 20ms when there's nothing to do.

Against `cargo generate-lockfile` on dedicated projects, the lockfiles are byte for byte identical for: a used path patch plus unused path and git patches; a `[replace]` entry; and a `resolver = "3"` project with an old `rust-version` (40 packages, several of them held back, and the lockfile written in the older format Cargo picks for that Rust version). Getting there took two corrections that reading Cargo's documentation hadn't suggested: patches are *preferred* to newer published versions, and the Rust version preference applies across compatibility buckets.

Automated coverage:

- Unit tests for the requirement semantics, the manifest reader and editor, the index parser, the solver rules (against an in-memory catalog), the lockfile writer, and the toolchain manifests.
- Integration tests (`features/cargo.test.ts`) against a mock sparse registry served by the fixture server, with a token-protected second registry: resolution, features, yanked versions, checksums, seeding, other registries and authentication, patches, replacements, git dependencies with submodules, and the commands. They ask Cargo to confirm the result whenever the machine has it.
- An e2e test (`tests/e2e/rust.sh`) against crates.io, on a machine without Rust: the toolchain is downloaded, and the project built and run through `yarn cargo`.

## Not covered yet

1. **Git-based registry indexes.** Only the sparse protocol is implemented; crates.io is reached through its sparse index.
2. **Disk usage.** The vendor directory holds every crate of every platform (541 MB here), as directory sources require. Unpacking once in the global folder and hardlinking (like the venv store), or switching to a local registry source, would both help.
3. **Features from the command line.** `yarn add` can't set the features of a dependency; `DependencyManifest::set` would need a way to carry ecosystem-specific options.
4. **Age gate.** The index reports publication times (`pubtime`), so a `cargoMinimalAgeGate` is a small addition. `packageRules` don't apply to crates either.
5. **`links`.** Two crates linking the same native library are detected after the fact, as an error, instead of being a constraint the solver can work around.
6. **Git crates in the cache.** They're used from the global folder, so an offline mirror doesn't cover them.
7. **`yarn info`** doesn't list crates (it reads the project-level resolutions, which islands aren't part of).
8. **Crates as workspaces.** `yarn workspaces foreach`, constraints and the release workflow don't see crates, which aren't Yarn workspaces (see the open questions).
9. **Windows and macOS** haven't been exercised; nothing is known to be Linux-specific (no symlinks are used), but the toolchain provisioning in particular is untested there.

## Building without Cargo

Driving `rustc` directly (what Buck and Bazel do) would make Yarn the build tool: `package.json` could then hold the dependencies, and `cargo:` ranges would work anywhere. It takes the per-target feature resolution Cargo runs at build time (the lockfile's is an over-approximation), `cfg()` evaluation, build scripts and their protocol (`cargo:rustc-cfg`, `links` metadata, `OUT_DIR`), proc-macros built for the host, profiles, and fingerprinting for incremental builds.

The rest of the Rust tooling isn't what stands in the way; both of the tools people rely on daily have a documented way in for build systems that aren't Cargo, and both were tried on a project using `serde` with its derive macros, with no Cargo on the machine:

- **rust-analyzer** reads a [`rust-project.json`](https://rust-analyzer.github.io/book/non_cargo_based_projects.html) describing the crate graph: root modules, editions, dependencies, cfgs, environment, sysroot. The linker has what it takes to write most of it (the features have to be computed again, as the lockfile doesn't store them), and rust-analyzer itself is a toolchain component Yarn can provision. Generated from `yarn.lock` and the vendored manifests alone, it gives a working project minus macro expansion: `#[derive(Serialize)]` is reported as an error. With the path of the compiled proc-macro and the output of the build scripts (`OUT_DIR`, the cfgs they emit) filled in, the project loads clean.
- **clippy** doesn't need a file: `clippy-driver` takes the arguments `rustc` takes, so whatever invokes the compiler can invoke it instead. It needs the compiled metadata of the crate's dependencies, like the compiler does. Check-on-save in rust-analyzer is a `flycheck` entry of `rust-project.json` naming a command that prints the compiler's JSON diagnostics.

So the files can be generated, but what makes them useful are build products - compiled proc-macros (and what they depend on: `syn`, `quote`, `proc-macro2`), and build script outputs - which only exist once something drove the compiler. That makes building build scripts and proc-macro crates for the host the natural first slice of a Cargo-less build: it's the part with the most protocol to get right, and it's enough to tell whether the rest is worth it. `cargo test`, `cargo bench` and third-party Cargo subcommands would have no equivalent until rebuilt.

Provisioning Cargo gets users the same result today (nothing to install, `yarn cargo build` works) with the whole ecosystem working as-is, which is why it's what was implemented. The one thing still taken from the system is the C linker, which rustup doesn't provide either. One gap on that path: rust-analyzer looks for `cargo` in the `PATH` and in `~/.cargo/bin`, so an editor only finds a toolchain Yarn manages when it's started with it in its `PATH`.

## Open questions

- **Where artifacts go.** Yarn writes inside `.cargo/` (a block in `config.toml`, and `vendor/`, which has to be git-ignored). Is editing `.cargo/config.toml` acceptable? The alternative is to have `yarn cargo` pass the configuration to Cargo (`--config`), at the cost of plain `cargo` and rust-analyzer not seeing the vendored crates.
- **Should `Cargo.lock` be committed?** It's derived, so it could be ignored like `.pnp.cjs` often is; keeping it makes the repository usable without Yarn, and diffs readable to Rust developers.
- **Should crates be Yarn workspaces?** They aren't required to be, which keeps adoption to three lines of configuration. Making them workspaces would bring `yarn workspaces foreach`, constraints and the release workflow to crates, at the cost of a `package.json` per crate.
- **Edges between ecosystems.** A JavaScript workspace wrapping a crate (napi, wasm) can't express that dependency today; islands are separate graphs.
