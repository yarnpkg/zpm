# Python conditional dependency resolution

This document explains how to add support for Python dependency markers in the
island resolver. It focuses on marker-conditioned dependency forks, following the
design where conditionality is represented by env-qualified logical locators
rather than by adding a separate `conditional_dependencies` field to
`Resolution`.

The intended implementation model is:

```text
foo@env:<linux-hash>#pypi:1.0.0
  dependencies:
    bar -> bar@env:<linux-hash>#pypi:>=2.0.0

foo@env:<windows-hash>#pypi:1.0.0
  dependencies:
    bar -> bar@env:<windows-hash>#pypi:<2.0.0
```

Each env-qualified `foo` is a normal `Resolution`. Its dependencies are normal
`BTreeMap<Ident, Descriptor>` entries. The marker has already been evaluated
when that fork-specific resolution was created.

## Current state

Yarn/ZPM already has a few pieces of Python support:

- `pypi:` ranges are parsed in `packages/zpm-primitives/src/range.rs`.
- PyPI locators are represented by `PypiShorthand` and `PypiRegistry` in
  `packages/zpm-primitives/src/reference.rs`.
- `packages/zpm/src/resolvers/pypi.rs` can resolve a PyPI range to a wheel and
  convert simple `requires_dist` entries into normal dependencies.
- `packages/zpm/src/linker/venv.rs` can link an island workspace into a
  `.venv/lib/site-packages` tree.

The missing pieces are significant:

- `parse_requires_dist_entry` currently drops any requirement containing `;`.
  Marker-bearing dependencies never enter the dependency graph.
- `Resolution.dependencies` is a `BTreeMap<Ident, Descriptor>`, so it cannot
  represent multiple marker-conditioned dependencies with the same ident.
- The island PubGrub provider treats only npm semver ranges as native version
  sets. PyPI specifier ranges are resolved eagerly and injected as exact
  singletons.
- `IslandPackage::Named(Ident)` conflates packages with the same ident from
  different registries and forks.
- The venv linker assumes at most one physical locator per package ident after
  traversal.

## Goals

- Support PEP 508 environment markers in PyPI `Requires-Dist` metadata.
- Resolve all supported Python target environments into a single lockfile.
- Keep `Resolution.dependencies` unchanged.
- Represent fork-specific graph shape using env-qualified logical locators and
  descriptors.
- Keep fetching and cache identity tied to the physical inner locator whenever
  possible.
- Keep configured islands as the user-facing isolation unit. A single configured
  island may contain multiple resolver forks.
- Use the project-level `supportedTargets` matrix as the source of Python target
  environments. `supportedArchitectures` remains a compatibility shorthand when
  `supportedTargets` is absent.

## Non-goals

- Do not add `conditional_dependencies` to `Resolution`.
- Do not hand-roll a full Python package installer. The first version should
  continue to install wheels into the venv linker.
- Do not solve arbitrary symbolic marker expressions on day one. A concrete
  target-environment solve is acceptable as the first implementation.
- Do not make non-island PyPI resolution universal. The initial feature should
  target Python/venv islands, where the PubGrub provider owns the graph.
- Do not support requested Python extras in the first implementation.
- Do not make `@yarnpkg/python` the v1 source of target environments. Keep the
  model compatible with such a runtime source later, but start with declarative
  targets.

## Terminology

- **Configured island**: The user-configured island from `unstableIslands`.
- **Fork**: A resolver domain inside an island. Each fork corresponds to a
  Python target environment or a normalized marker domain.
- **Python target environment**: Values used to evaluate PEP 508 markers, such
  as `python_version`, `sys_platform`, `platform_machine`, and
  `implementation_name`.
- **Supported target**: A project-level target entry. It contains a typed
  `System` shape (`os`, `cpu`, `libc`) plus optional ecosystem-specific data
  such as `python.version`.
- **Env-qualified locator**: A logical locator whose reference wraps another
  reference with a fork id, for example `foo@env:<hash>#pypi:1.0.0`.
- **Physical locator**: The locator used for fetching package bytes. For an
  env-qualified locator, this is the inner locator with the `env:` wrapper
  stripped.
- **Runtime source**: A future mechanism, such as `@yarnpkg/python`, that can be
  pre-resolved into concrete Python target environments before PyPI solving.

## Core design

### Env-qualified references

Add an `Env` reference wrapper to `Reference`, similar in spirit to `Virtual`:

```rust
Reference::Env {
    hash: Hash64,
    inner: Box<Reference>,
}
```

Suggested serialization:

```text
env:<hash>#<inner-reference>
```

Examples:

```text
foo@env:5ee3b4#pypi:1.0.0
bar@env:aa18ff#pypi:bar@2.0.0#https%3A%2F%2F...
workspace-a@env:5ee3b4#workspace:workspace-a
```

The `env:` wrapper is a logical identity wrapper. It must affect resolver and
lockfile identity, but it should be stripped by physical package operations:

- `Locator::physical_locator`
- `Reference::physical_reference`
- fetcher dispatch
- package cache lookup
- content flag extraction when the package bytes are identical

`Virtual` and `Env` should compose. The exact order should be normalized by
helpers so the same logical package always serializes the same way. Prefer:

```text
virtual:<peer-hash>#env:<fork-hash>#<inner-reference>
```

because peer virtualization is already a tree-resolver concern layered on top
of resolved packages.

### Env-qualified ranges

Add a matching `Env` range wrapper to `Range`:

```rust
Range::Env {
    hash: Hash64,
    inner: Box<Range>,
}
```

Suggested serialization:

```text
env:<hash>#<inner-range>
```

This is important even if the main design talks about locators. The install
state and lockfile also contain descriptor-to-locator maps:

```rust
BTreeMap<Descriptor, Locator>
```

If descriptors are not env-qualified, two forks can write conflicting mappings
for the same descriptor. With env-qualified descriptors, these mappings can
coexist in a single map:

```text
bar@env:<linux>#pypi:>=1.0.0 -> bar@env:<linux>#pypi:2.0.0
bar@env:<win>#pypi:>=1.0.0   -> bar@env:<win>#pypi:1.0.0
```

Add helper methods rather than open-coding wrapper checks:

```rust
impl Range {
    pub fn env_qualified_with_hash(&self, hash: Hash64) -> Range;
    pub fn physical_range(&self) -> &Range; // extend the existing logic
}

impl Descriptor {
    pub fn env_qualified_with_hash(&self, hash: Hash64) -> Descriptor;
}

impl Locator {
    pub fn env_qualified_with_hash(&self, hash: Hash64) -> Locator;
}
```

### Fork metadata

The `env:` hash must be deterministic, but it should not be the only place
where the condition is stored. Store explicit fork metadata in the island
result and lockfile.

Suggested shape:

```rust
pub struct PythonFork {
    pub id: Hash64,
    pub condition: MarkerExpr,
    pub target: Option<PythonTargetEnv>,
}
```

For the first implementation, each fork can correspond to one concrete
`PythonTargetEnv`, and `condition` can be the exact marker expression for that
target. Later, the same structure can represent symbolic marker domains.

Compute the fork id from a canonical serialization of `PythonTargetEnv`, with a
schema/version tag such as `python-target-v1`. Do not derive the id from the
presentation condition string.

If multiple `supportedTargets` entries produce the same `PythonTargetEnv`,
dedupe them deterministically and silently before solving. Do not solve the same
fork twice.

Do not call these "islands" in code. Use "fork" or "environment fork" so we do
not confuse them with configured resolution islands.

## Marker model

### Parse markers

Use the existing `pep-508` dependency to parse `Requires-Dist`. The parser
already returns:

```rust
pep_508::Dependency {
    name,
    extras,
    spec,
    marker,
}
```

Do not keep using ad-hoc string splitting in `parse_requires_dist_entry`.
Replace it with a parser that returns a richer internal representation:

```rust
pub struct PypiRequirement {
    pub ident: Ident,
    pub descriptor: Descriptor,
    pub marker: MarkerExpr,
}
```

For requirements without a marker, use `MarkerExpr::Any`.

Do not collect parsed requirements directly into a `BTreeMap<Ident,
Descriptor>`. Multiple active requirements for the same canonical PyPI package
can appear in real metadata. After marker evaluation for a fork, group active
requirements by canonical PyPI ident and intersect their specifier sets. If the
intersection is empty, let the solver surface the conflict.

Canonicalize PyPI package names before creating `Ident` values:

1. Lowercase the name.
2. Collapse runs of `-`, `_`, and `.` into `-`.
3. Use the canonical name for graph identity, lockfile descriptors, and
   same-ident merging.

Handle unsupported requirement forms intentionally, with explicit behavior:

- direct URL requirements
- requested dependency extras such as `foo[bar]`
- pure optional-extra requirements guarded by `extra`, which are inactive because
  v1 has no requested extras model
- mixed marker expressions involving `extra`, which should report unsupported
  until extras are modeled deliberately

Do not silently drop unsupported marker expressions forever. The parser should
eventually return enough information to report unsupported cases clearly.

### Own the marker AST

Do not store `pep_508::Marker<'a>` directly in long-lived structs. It borrows
from the parsed string and is not designed as our lockfile representation.

Add an owned, serializable marker AST, likely under `zpm-primitives`:

```rust
pub enum MarkerExpr {
    Any,
    Never,
    And(Box<MarkerExpr>, Box<MarkerExpr>),
    Or(Box<MarkerExpr>, Box<MarkerExpr>),
    Not(Box<MarkerExpr>),
    Compare {
        variable: MarkerVariable,
        op: MarkerOp,
        value: String,
    },
}
```

Use names that match PEP 508 marker variables:

- `python_version`
- `python_full_version`
- `os_name`
- `sys_platform`
- `platform_machine`
- `platform_system`
- `platform_release`
- `platform_version`
- `platform_python_implementation`
- `implementation_name`
- `implementation_version`
- `extra`

The first implementation only needs boolean evaluation against concrete target
environments. Marker comparison semantics should be explicit:

- Support `==`, `!=`, `<`, `<=`, `>`, `>=`, `in`, and `not in`.
- Use Python-version comparison semantics for `python_version`,
  `python_full_version`, and `implementation_version`.
- Use string comparison semantics for platform fields.
- If a marker references a field the active target cannot provide, return a
  clear unsupported/incomplete-target error rather than guessing false.

Symbolic operations can be added later:

- normalize
- hash
- intersection
- difference
- satisfiability
- implication

### Target environments

Add a `SupportedTarget` / `TargetEnv` model at the configuration boundary. It
should contain a typed `System` plus ecosystem-specific payloads:

```rust
pub struct SupportedTarget {
    pub system: zpm_utils::System,
    pub python: Option<PythonTarget>,
}

pub struct PythonTarget {
    pub version: String,
    pub full_version: Option<String>,
    pub implementation_name: Option<String>,
    pub implementation_version: Option<String>,
    pub platform_release: Option<String>,
    pub platform_version: Option<String>,
}
```

Derive a `PythonTargetEnv` from `SupportedTarget` when resolving a Python island:

```rust
pub struct PythonTargetEnv {
    pub python_version: String,
    pub python_full_version: Option<String>,
    pub os_name: Option<String>,
    pub sys_platform: Option<String>,
    pub platform_machine: Option<String>,
    pub platform_system: Option<String>,
    pub platform_release: Option<String>,
    pub platform_version: Option<String>,
    pub platform_python_implementation: Option<String>,
    pub implementation_name: Option<String>,
    pub implementation_version: Option<String>,
}
```

Use string values that match Python packaging expectations, not Yarn's `Os` and
`Cpu` enum names. Provide conversion helpers from `zpm_utils::System` where
reasonable:

- `sys_platform`: `linux`, `darwin`, `win32`, or an explicit unsupported error
  for unknown OS values.
- `os_name`: `posix` for Linux/macOS, `nt` for Windows.
- `platform_system`: `Linux`, `Darwin`, `Windows`, or unsupported for unknown OS
  values.
- `platform_machine`: derived from `cpu` where the mapping is known.
- `python_version`: `python.version`.
- `python_full_version`: `python.fullVersion` if present, otherwise
  `python.version`.
- `implementation_name`: `python.implementationName` if present, otherwise
  `cpython`.
- `platform_python_implementation`: derived from `implementation_name`, default
  `CPython`.
- `implementation_version`: `python.implementationVersion` if present,
  otherwise `python.fullVersion` or `python.version`.
- `platform_release` and `platform_version`: explicit target fields only; error
  if a marker needs them and they are missing.

Project configuration should use a global `supportedTargets` matrix:

```yaml
supportedTargets:
  - os: linux
    cpu: x64
    libc: glibc
    python:
      version: "3.12"
      implementationName: cpython
  - os: darwin
    cpu: arm64
    python:
      version: "3.12"

unstableIslands:
  python:
    workspaces:
      - "@acme/py-*"
    linker: venv
```

If `supportedTargets` is non-empty, it wins. If it is absent, derive targets
from `supportedArchitectures.to_systems()` for compatibility with existing
platform-aware package resolution.

For Python islands, any marker-bearing PyPI dependency requires every active
target used by that island to have a `python.version`. Plain non-marker PyPI
dependencies can keep working without Python target data, but marker evaluation
must not invent a Python version.

## Resolver changes

### Make island package identity registry-aware

`IslandPackage::Named(Ident)` is too small. It cannot distinguish npm `foo`,
PyPI `foo`, and workspace `foo`, and it cannot distinguish fork-local packages
in a symbolic solve.

For PyPI packages, the `ident` stored in the package key must be the canonical
PyPI name, not the raw spelling from metadata or user input.

Introduce an explicit package key:

```rust
pub enum IslandRegistry {
    Npm,
    Pypi,
    Workspace,
    Other,
}

pub struct IslandPackageKey {
    pub ident: Ident,
    pub registry: IslandRegistry,
}

pub enum IslandPackage {
    Root,
    Named(IslandPackageKey),
}
```

For the first concrete-target implementation, the provider itself is
fork-specific, so `IslandPackage` does not need to carry `fork_id`. If we later
move to a single symbolic PubGrub run, include `fork_id` in the package key.

Aliases remain dependency-edge metadata; they do not create independent
resolution slots. Within a fork, all descriptors that reference the same
canonical ecosystem package contribute constraints to the same
`IslandPackageKey`. Those constraints must be intersected rather than replaced.
Incompatible alias constraints therefore produce a normal PubGrub conflict,
and aliases cannot be used to install multiple versions of the same source
package inside an island.

### Add PyPI version sets

`IslandVersionSet` needs a PEP 440 variant:

```rust
pub enum IslandVersionSet {
    Semver(Ranges<PubgrubVersion>),
    Pypi(PypiSpecifierSet),
    Exact(ExactSet),
}
```

Then update:

- `VersionSet::contains`
- `VersionSet::intersection`
- `VersionSet::is_disjoint`
- `VersionSet::subset_of`
- `VersionSet::complement`

PEP 440 complement and subset logic can be difficult. If the existing
`pep440_rs` APIs are insufficient, start with a conservative finite-candidate
approach inside the PyPI branch of the provider:

1. Fetch candidate versions from project metadata.
2. Filter by all accumulated specifiers.
3. Let PubGrub exclusions remove concrete candidate versions.

If that becomes awkward with the `VersionSet` trait, introduce a dedicated
finite `ExactSet` for PyPI candidates after metadata is known. The important
part is that PyPI ranges must not be resolved to a single version before
PubGrub has seen all constraints.

### Resolve versions by registry

Extend `Registry` and `resolve_versions` so the provider can fetch PyPI
versions:

```rust
Registry::Pypi(Ident)
```

`resolvers::pypi` should expose a metadata function that returns candidate
locators without selecting a wheel yet:

```rust
pub async fn resolve_versions(
    context: &InstallContext<'_>,
    ident: &Ident,
) -> Result<Vec<Locator>, Error>
```

The returned locators should be physical PyPI locators. The island provider
will wrap them with `env:` after selecting a version for a fork.

### Build fork-specific resolutions

The PyPI resolver needs a fork-aware path. Conceptually:

```rust
pub async fn resolve_locator_for_fork(
    context: &InstallContext<'_>,
    locator: &Locator,
    params: &PypiRegistryReference,
    fork: &PythonFork,
) -> Result<ResolutionResult, Error>
```

This function should:

1. Fetch version metadata.
2. Parse `requires_dist` into `PypiRequirement` values.
3. Evaluate each marker against `fork.target`.
4. Keep only active requirements.
5. Group active requirements by canonical PyPI ident.
6. Intersect same-ident specifier sets before creating the dependency map.
7. Convert each active dependency descriptor into an env-qualified descriptor
   for the same fork.
8. Return a normal `Resolution` whose locator is env-qualified and whose
   `dependencies` field contains only unconditional, active dependencies.

For a package with different dependencies by platform:

```text
Requires-Dist: bar (<2.0.0); sys_platform == "win32"
Requires-Dist: bar (>=2.0.0); sys_platform == "linux"
```

the fork-specific resolutions become:

```text
foo@env:<win>#pypi:1.0.0
  dependencies:
    bar: bar@env:<win>#pypi:<2.0.0

foo@env:<linux>#pypi:1.0.0
  dependencies:
    bar: bar@env:<linux>#pypi:>=2.0.0
```

No `conditional_dependencies` field is required.

### Workspace resolutions are fork-specific too

Workspace packages in Python islands must also be env-qualified. A workspace's
manifest dependencies may be unconditional at the manifest level, but its
transitive dependencies may not be. More importantly, the root workspace
resolution is the entry point into a fork-specific graph.

For each fork, generate:

```text
workspace-a@env:<fork>#workspace:workspace-a
```

with dependencies rewritten to env-qualified descriptors:

```text
some-pypi-dep: some-pypi-dep@env:<fork>#pypi:>=1.0.0
```

## Island resolution flow

### First implementation: solve per concrete target

The recommended first implementation is one PubGrub solve per concrete Python
target derived from `supportedTargets`.

For each configured island:

1. Compute the set of Python forks from the project target matrix.
2. For each fork, create an `IslandDependencyProvider` with that fork in its
   state.
3. Build fork-specific root deps from env-qualified workspace locators.
4. Run PubGrub.
5. Convert the solution into env-qualified descriptors, locators, and
   resolutions.
6. Merge the fork results into the island result.

This is easier than symbolic fork splitting because all marker evaluation is
boolean. It is also enough to support universal lockfiles for a finite set of
Python targets.

### Later implementation: symbolic fork splitting

After the concrete-target implementation works, we can optimize toward uv-style
forking:

1. Start with one broad marker domain.
2. When a package introduces a marker that partially overlaps the current
   domain, split the domain.
3. Continue solving each resulting fork.
4. Merge forks whose selected versions and dependencies are identical.

This requires marker algebra beyond boolean evaluation. It should not be the
first milestone.

## Lockfile changes

The existing lockfile has:

```rust
pub islands: BTreeMap<String, BTreeMap<Descriptor, Locator>>
```

That is not enough to preserve fork metadata. Replace the island payload with
a structured value:

```rust
pub struct Lockfile {
    pub islands: BTreeMap<String, LockfileIsland>,
}

pub struct LockfileIsland {
    pub forks: BTreeMap<Hash64, LockfileIslandFork>,
}

pub struct LockfileIslandFork {
    pub condition: MarkerExpr,
    pub target: Option<PythonTargetEnv>,
    pub resolutions: BTreeMap<Descriptor, Locator>,
}
```

The serialized lockfile should keep normal lockfile entries for env-qualified
logical locators, because two forks may use the same physical package version
with different fork-local dependencies.

Example shape:

```yaml
islands:
  python:
    forks:
      5ee3b4:
        condition: "python_version == '3.11' and sys_platform == 'linux'"
        entries:
          "workspace-a@env:5ee3b4#workspace:workspace-a":
            resolution: "workspace-a@env:5ee3b4#workspace:workspace-a"
            dependencies:
              foo: "env:5ee3b4#pypi:>=1.0.0"
          "foo@env:5ee3b4#pypi:1.0.0":
            resolution: "foo@env:5ee3b4#pypi:1.0.0"
            dependencies:
              bar: "env:5ee3b4#pypi:>=2.0.0"
```

The exact YAML layout can follow existing `MultiKey<Descriptor>` conventions,
but the fork boundary must be represented explicitly. Bump
`LOCKFILE_VERSION`.

Store generated fork metadata in the lockfile, not raw `supportedTargets`
configuration. The lockfile should describe the concrete forks it solved; the
project config remains the source for regenerating those forks.

### Checksums

Env-qualified locators are logical. Their package bytes come from their
physical locator. There are two viable checksum strategies:

1. Store the same physical checksum on each env-qualified logical entry.
2. Store checksums only for physical package cache entries and teach lockfile
   validation to look through `physical_locator`.

The first strategy is simpler. The second is cleaner and avoids duplicate
checksum churn. Start with the simpler strategy unless it causes visible
lockfile noise.

## Fetching and package data

Fetchers should operate on physical locators. Add helper methods so install
code can consistently do this:

```rust
let fetch_locator = locator.physical_locator();
```

The fetch result can still be associated with the logical locator if later
install phases need it, but package bytes and cache paths should be keyed by
the physical locator. This matches how the venv linker already looks up package
data via `physical_locator`.

Be careful with `ContentFlags`. Entry points are read from package bytes, so
they can usually be shared across forks for the same physical locator. If a
future Python package format exposes marker-conditioned entry points, that can
be revisited separately.

## Venv linker changes

The venv linker must select exactly one active fork for the current Python
environment before traversing dependencies.

Suggested flow:

1. Determine the active `PythonTargetEnv`.
2. Find the configured island fork whose target matches that environment.
3. Start traversal from the env-qualified workspace locator for that fork.
4. Traverse only env-qualified descriptors and locators for that fork.
5. Link physical package bytes into `.venv/lib/site-packages`.

For v1, the active target is selected from the current `System` plus an optional
island setting:

```yaml
unstableIslands:
  python:
    linker: venv
    python:
      linkVersion: "3.12"
```

`linkVersion` affects linking only. It must match one of the Python forks
generated from `supportedTargets` for the current `System`. Resolution still
solves every supported target. If exactly one fork matches the current `System`,
`linkVersion` is not required. If multiple Python versions match, the venv linker
must error unless `linkVersion` selects one of them.

The existing duplicate-ident check can remain after active-fork filtering:

```rust
if let Some(existing_locator) = packages.get(&physical_locator.ident) {
    if existing_locator != &physical_locator {
        return Err(Error::Unsupported);
    }
}
```

Before filtering, multiple locators for the same ident are expected. After
filtering to a concrete fork, they should not conflict.

## Wheel and `Requires-Python` filtering

Conditional dependency support will make existing PyPI candidate selection
more visible. The implementation should also account for:

- `requires_python` on project/release metadata
- wheel tags compatible with the target Python environment
- platform-specific wheels

Current wheel selection chooses the newest wheel by upload time. That is not
enough for multi-environment resolution. Add target-aware wheel selection:

```rust
select_best_wheel(distributions, &python_target_env)
```

For the first implementation, it is acceptable to support `py3-none-any`
universal wheels and report unsupported for platform wheels that cannot be
matched safely. Do not pick an arbitrary incompatible wheel.

## Configuration

Add a global `supportedTargets` setting. It is the project-level target universe
used by Python forks and any future ecosystem that needs more than
`supportedArchitectures` can express.

`supportedTargets` wins when present. If it is absent, derive targets from
`supportedArchitectures.to_systems()` so existing projects and builtin package
variants keep their current behavior.

Possible schema:

```json
{
  "supportedTargets": [
    {
      "os": "linux",
      "cpu": "x64",
      "libc": "glibc",
      "python": {
        "version": "3.12",
        "implementationName": "cpython"
      }
    },
    {
      "os": "darwin",
      "cpu": "arm64",
      "python": {
        "version": "3.12"
      }
    }
  ],
  "unstableIslands": {
    "python": {
      "workspaces": ["@acme/py-*"],
      "linker": "venv",
      "python": {
        "linkVersion": "3.12"
      }
    }
  }
}
```

Do not put Python versions under `IslandDefinition` in v1. A venv island reads
the global target matrix and uses the target entries that have a `python`
payload. A target without `python.version` is incomplete for marker-bearing PyPI
metadata.

An island-level `python.linkVersion` setting is allowed, but it only selects the
fork materialized by the venv linker on the current machine. It does not limit
resolution and it is only required when multiple Python versions match the
current `System`.

### Future `@yarnpkg/python` runtime source

The target model should leave room for a future builtin runtime source. In that
model, a project or island could request something like
`@yarnpkg/python@builtin:^3.12`; ZPM would pre-resolve the runtime into concrete
Python target data before PyPI island solving begins.

Do not implement that bootstrap in v1. The important design constraint is that
`PythonTargetEnv` is independent from how the target was produced: declarative
`supportedTargets` now, runtime pre-resolution later.

## Migration strategy

1. Bump the lockfile version.
2. Continue deserializing older island lockfiles into a single default fork.
3. When writing a new lockfile, always write the structured fork payload.
4. Keep non-island `pypi:` behavior unchanged at first, except for replacing
   ad-hoc parsing with the shared PEP 508 parser where safe.

Older lockfiles should not be silently interpreted as universal Python locks.
If a project configures multiple Python targets, a pre-fork lockfile should be
considered incomplete and re-resolved.

## Implementation order

### Phase 1: Primitive wrappers

Files:

- `packages/zpm-primitives/src/reference.rs`
- `packages/zpm-primitives/src/range.rs`
- `packages/zpm-primitives/src/locator.rs`
- `packages/zpm-primitives/src/descriptor.rs`
- `packages/zpm/src/primitives_exts.rs`

Tasks:

- Add `Reference::Env`.
- Add `Range::Env`.
- Add serialization tests for env references, env ranges, locators, and
  descriptors.
- Update `physical_reference`, `physical_locator`, and `physical_descriptor`.
- Update range details, inner-descriptor handling, fetch dispatch, workspace
  checks, and slug/content-flag paths to use env physical helpers.
- Add helper constructors for env-qualified descriptors and locators.

### Phase 2: Marker and target environment model

Files:

- `packages/zpm-primitives/src/pypi.rs` or a new marker module
- `packages/zpm-config/schema.json`
- `packages/zpm-config/src/types.rs`

Tasks:

- Add owned marker types.
- Add marker parsing from `pep_508`.
- Add marker evaluation against `PythonTargetEnv`.
- Add deterministic marker/fork hashing.
- Add global `supportedTargets` config.
- Add fallback derivation from `supportedArchitectures`.
- Add target-to-PEP-508 conversion helpers with incomplete-target errors.
- Hash fork ids from canonical `PythonTargetEnv` data.
- Dedupe duplicate `PythonTargetEnv` values before solving.

### Phase 3: PyPI metadata parsing

Files:

- `packages/zpm/src/resolvers/pypi.rs`
- `tests/acceptance-tests/pkg-tests-core/sources/utils/tests.ts`

Tasks:

- Replace marker-dropping `parse_requires_dist_entry`.
- Parse all `Requires-Dist` entries into `PypiRequirement`.
- Canonicalize PyPI names before creating graph idents.
- Reject requested extras and handle `extra` marker guards intentionally.
- Group active same-ident requirements and intersect specifier sets.
- Keep unsupported entries explicit.
- Add tests proving marker-bearing requirements are retained internally.

### Phase 4: PyPI-aware island provider

Files:

- `packages/zpm/src/island_types.rs`
- `packages/zpm/src/island_provider.rs`
- `packages/zpm/src/island.rs`
- `packages/zpm/src/resolvers/mod.rs`
- `packages/zpm/src/resolvers/pypi.rs`

Tasks:

- Make island package identity registry-aware.
- Add PyPI version listing.
- Add PyPI version-set support or a finite-candidate fallback.
- Add fork state to `IslandDependencyProvider`.
- Rewrite descriptors to env-qualified descriptors inside each fork.
- Return env-qualified locators and resolutions from `convert_solution`.

### Phase 5: Lockfile and install state

Files:

- `packages/zpm/src/lockfile.rs`
- `packages/zpm/src/install.rs`

Tasks:

- Replace per-island map payload with structured fork payload.
- Store fork metadata and fork-specific descriptor-to-locator maps.
- Store generated fork metadata rather than raw `supportedTargets`.
- Merge fork-local results without descriptor collisions.
- Fetch physical locators for all configured forks.
- Ensure checksum and package-data lookup paths use physical locators or
  explicit logical-to-physical aliases consistently.
- Preserve package data and checksum behavior.

### Phase 6: Venv linker

Files:

- `packages/zpm/src/linker/venv.rs`
- `packages/zpm/src/commands/python.rs`

Tasks:

- Determine the active Python environment.
- Add island-level `python.linkVersion` config for ambiguous local linking.
- Select the matching fork for a workspace.
- Traverse only that fork's env-qualified graph.
- Link physical package bytes into site-packages.
- Keep duplicate-ident validation after active-fork filtering.

### Phase 7: Tests

Add integration tests for:

- A marker-bearing dependency that resolves differently on Linux and Windows.
- A marker-bearing dependency that resolves differently across Python versions.
- Multiple dependencies with the same ident but disjoint markers.
- Multiple active dependencies with the same ident whose specifiers must be
  intersected.
- PyPI name canonicalization across case, hyphen, underscore, and dot spelling.
- `supportedTargets` generating Python forks and winning over
  `supportedArchitectures`.
- Duplicate `supportedTargets` entries deduping to a single Python fork.
- Fork ids remaining stable for identical canonical `PythonTargetEnv` values.
- Marker-bearing PyPI metadata failing clearly when the active target lacks
  `python.version`.
- Lockfile stability when generated on one host and consumed on another.
- Venv linking selecting the active fork.
- Venv linking requiring `python.linkVersion` only when multiple Python versions
  match the current `System`.
- Physical package cache reuse across env-qualified locators.
- Unsupported direct URL requirements with markers.
- Unsupported requested extras.
- `Requires-Python` rejecting incompatible versions.
- Universal `py3-none-any` wheels working across configured forks.

The existing test that expects marker-bearing requirements to be ignored should
be replaced with one that expects inactive marker requirements to be absent from
the active fork, while active marker requirements are present in their matching
fork.

## Open questions

- Should env-qualified logical entries duplicate checksums in the lockfile, or
  should checksums be stored only for physical locators?
- How much wheel tag compatibility should the first version support?
- Should non-island `pypi:` dependencies evaluate markers against the current
  host immediately, or should marker support remain island-only until the fork
  model is available everywhere?
- What exact schema should a future `@yarnpkg/python` runtime source use, and
  does it live globally or under each Python island?
- Should future target selection allow per-island filtering of global
  `supportedTargets`?

## Summary

The key idea is to compile conditional Python metadata into fork-specific
ordinary resolutions. The condition lives on the fork and in the `env:` wrapper,
not on the dependency edge.

That gives us these properties:

- `Resolution.dependencies` stays unchanged.
- The tree resolver can keep traversing normal dependency maps.
- Lockfiles can store a universal Python graph by keeping fork boundaries.
- Fetching remains deduplicated by physical locators.
- The venv linker can select the active fork and install a normal concrete
  environment.

## Implementation debt (post-review)

Items surfaced by the design review of the initial implementation. Each either
needs a maintainer decision or touches the lockfile schema, so they are
recorded here rather than fixed opportunistically. File pointers reference the
state at the time of the review.

- **Step-wise PubGrub driver.** `resolve_island_once` bridges the synchronous
  `pubgrub::resolve` to async fetching via `spawn_blocking` plus an `unsafe`
  transmute of `InstallContext` to `'static` (`island.rs`). The transmute is
  only sound while every caller polls the island futures to completion; the
  awaits in `install.rs` and `island.rs` now use non-cancelling `join`/
  `join_all` combinators and the invariant is documented at the transmute, but
  the durable fix is the TODO already noted in `island.rs`: drive PubGrub
  step-by-step (`unit_propagation`/`add_decision`) from an async loop that
  owns its data, which also unlocks concurrent metadata prefetching.
- **Placeholder fetches as a type, not a flag.** `is_mock_request` conflates
  "foreign architecture" and "inactive Python fork", and placeholder-ness is
  only visible as `PackageData::MissingZip` (see `is_missing_zip` and the
  write guard in `record_fetch`). A `Materialized`/`Placeholder` split on
  fetch results, with package data keyed by physical locator plus a
  logical→physical alias table, would remove both the write-side guard and
  the read-side re-fetch convention (`install.rs`, patch path).
- **Checksum completeness for inactive forks.** The late-checksum pass only
  hashes materialized zips, so lockfile entries for forks inactive on the
  resolving host are written with `checksum: null` and get filled in later by
  whichever teammate's host activates them — churning the lockfile and
  conflicting with the universal-lockfile goal (and with the cross-host
  stability test listed above). Options: carry the registry-declared artifact
  hash through resolution so every fork entry is complete at resolve time, or
  store checksums keyed by physical locator (see the first open question).
- **One marker evaluator.** Live marker evaluation goes through `pep508_rs`
  (`resolvers/pypi.rs`), while the owned `MarkerExpr` AST in
  `zpm-primitives/src/pypi_marker.rs` — which this document assigns to the
  lockfile fork representation — is constructed for `PythonFork.condition`
  but never serialized or read. Either serialize `condition` into
  `LockfileIslandFork` and route evaluation through `MarkerExpr`, or delete
  the AST and canonicalize on `pep508_rs`; keeping both invites divergence.
- **Post-solve fork coalescing.** Fork results are merged by map extension,
  so every package appears once per fork even when all forks resolved
  identically; lockfile size and diff noise scale with
  `targets × packages`. Collapsing entries whose physical locator and
  dependency set match across all forks requires no marker algebra and
  should land before the format is widely adopted.
- **Island lockfile fast-path.** Disabled (`island.rs` TODO); every install
  re-runs PubGrub per fork with locked versions only as preferences. The
  validation helpers (`is_island_lockfile_valid`,
  `island_result_from_lockfile`) are `#[allow(dead_code)]` and should be
  finished or removed before islands leave the unstable flag.
- **Resolver-mode families.** The `resolve_*` / `*_requiring_python_target` /
  `*_for_fork` function triplets in `resolvers/pypi.rs` should collapse into
  single functions over a resolution-mode enum (the pattern already exists
  internally as `LocalWheelResolutionTarget`).
- **Lossy PEP 440 projection.** `Resolution.version` stores a lossy
  semver projection of PEP 440 versions (`project_pep440_to_semver`); the
  projected form is lockfile-visible, so the eventual fix needs a
  migration story (comment at the projection site).
