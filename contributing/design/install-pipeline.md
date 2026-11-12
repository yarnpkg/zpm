# Install pipeline

The install pipeline is the core of the package manager. It's responsible for resolving the dependencies, fetching the packages, and linking the project.

## Primitives

The primitives are minimal data structures reused by the rest of the software. The main ones are:

- `Ident`: A package identifier (also called its name).
- `Descriptor`: An `Ident` coupled with `Range`. References multiple packages.
- `Locator`: An `Ident` coupled with a `Reference`. References a single package.

## Core

The install is implemented in three different files:

- [`graph.rs`](../../packages/zpm/src/graph.rs): A generic implementation of a graph taskmaster. You push tasks inside it and the graph implementation will retrieve their dependencies, ensure they are processed first, then execute the async task, then collect its result and self-dispatch follow-up tasks. It uses traits to configure all that.

- [`install.rs`](../../packages/zpm/src/install.rs): The main install implementation. It contains the graph trait implementations relevant to the install loop. See for instance the `InstallOp` enum to see which kind of operations our graph consumes, and `InstallOpResult` to see what it returns.

- [`tree_resolver.rs`](../../packages/zpm/src/tree_resolver.rs): Called once the install operations are complete, the tree resolver will traverse the dependency tree and resolve then dedupe all peer dependencies by generating new virtual packages wherever necessary.

## Project orchestration

`Project` exposes preparation separately from finalizing an install:

- `prepare_install` validates the project, loads its lockfile, and resolves and fetches dependencies. It can recover lockfile conflicts using the regular install rules.
- `prepare_install_from_lockfile` resolves and fetches from an in-memory lockfile, allowing callers to apply enforced resolutions without writing an intermediate lockfile.
- `Install::link_and_build` finalizes the result according to the selected install mode. It writes the lockfile and, when linking is enabled, cleans the package cache and attaches the installed state.

Preparation can populate caches, but does not clean them, link packages, or persist a new installed state. Each resolution drains its background metadata writes before returning, including on failure. Callers own the report and share one package cache across these stages so cache cleanup retains packages accessed by either resolution. Lockfile-only finalization skips cleanup to preserve archives still referenced by the previous linked tree.

`Project::run_install` orchestrates normal installs, including optional automatic deduplication.

## Resolvers

Resolvers are responsible for turning descriptors into locators. They're all located in the [`resolvers`](../../packages/zpm/src/resolvers) directory.

Resolvers are most often async, but sync resolvers can also be defined when you're sure you can quickly access the relevant information (like is the case for workspaces, for example). Doing this squeezes some extra performances by avoiding to allocate new futures.

## Fetchers

Fetchers are responsible for retrieving the packages data from whatever source is appropriate. They're all located in the [`fetchers`](../../packages/zpm/src/fetchers) directory.

## Linkers

Linkers are responsible for taking all packages we resolved and fetched, and turning them into concrete install artifacts. The PnP linker will thus generate a `.pnp.cjs` file, whereas the `node-modules` linker will generate a bunch of ... `node-modules` folders.
