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

## Resolvers

Resolvers are responsible for turning descriptors into locators. They're all located in the [`resolvers`](../../packages/zpm/src/resolvers) directory.

Resolvers are most often async, but sync resolvers can also be defined when you're sure you can quickly access the relevant information (like is the case for workspaces, for example). Doing this squeezes some extra performances by avoiding to allocate new futures.

## Fetchers

Fetchers are responsible for retrieving the packages data from whatever source is appropriate. They're all located in the [`fetchers`](../../packages/zpm/src/fetchers) directory.

## Linkers

Linkers are responsible for taking all packages we resolved and fetched, and turning them into concrete install artifacts. The PnP linker will thus generate a `.pnp.cjs` file, whereas the `node-modules` linker will generate a bunch of ... `node-modules` folders.
