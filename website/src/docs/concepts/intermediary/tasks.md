---
category: concepts
slug: concepts/tasks
title: Task dependencies
description: Define and manage task execution order with sequential and parallel dependencies, cross-workspace dependencies, and task includes.
sidebar:
  order: 6
---

Task dependencies allow you to define the execution order of tasks in your project. When running a task, Yarn automatically resolves and executes all required dependencies first, ensuring that prerequisites are met before each task starts.

## Defining tasks

Tasks are defined in a `taskfile` at the root of each workspace. Each task has a name, optional dependencies, and a script to execute:

```
build:
  echo "Building the project"

test: build
  echo "Running tests"
```

In this example, running `yarn test` will first execute `build`, then `test`.

## Sequential dependencies

By default, dependencies are sequential. Each dependency must complete before the next one starts:

```
lint:
  echo "Linting"

typecheck:
  echo "Type checking"

build: lint typecheck
  echo "Building"
```

When you run `yarn build`, the execution order is:
1. `lint` runs first
2. `typecheck` runs after `lint` completes
3. `build` runs after `typecheck` completes

This creates a strict ordering where each task waits for the previous one to finish.

## Parallel dependencies

To run dependencies in parallel, add the `&` suffix to the dependency name:

```
lint:
  echo "Linting"

typecheck:
  echo "Type checking"

build: lint& typecheck&
  echo "Building"
```

Now when you run `yarn build`:
1. `lint` and `typecheck` run simultaneously
2. `build` runs after both complete

Parallel execution can significantly speed up your build pipeline when tasks are independent of each other.

## Mixing sequential and parallel dependencies

You can combine sequential and parallel dependencies in a single task definition. Tasks with `&` form parallel groups, while tasks without `&` create sequential barriers:

```
a:
  echo "Task A"

b:
  echo "Task B"

c:
  echo "Task C"

d:
  echo "Task D"

e: a b& c& d
  echo "Task E"
```

The execution order for `yarn e` is:
1. `a` runs first (sequential barrier)
2. `b` and `c` run in parallel (both have `&`)
3. `d` runs after `b` and `c` complete (sequential barrier)
4. `e` runs after `d` completes

This pattern is useful when you have a setup task that must run first, followed by independent tasks that can run in parallel, and finally tasks that need all previous work to be done.

## Cross-workspace dependencies

Tasks can depend on tasks from other workspaces that are listed as dependencies in your `package.json`. Use the `workspace:task` syntax:

```
# In packages/app/taskfile
build: pkg-utils:build pkg-core:build
  echo "Building app"
```

This ensures that both `pkg-utils` and `pkg-core` are built before `app`. Note that `pkg-utils` and `pkg-core` must be declared as dependencies (or devDependencies) of `app` in its `package.json`.

You can also use glob patterns to match multiple dependency workspaces:

```
# Depend on all dependency packages matching the pattern
build: @my-scope/*:build
  echo "Building after all @my-scope packages"
```

The glob pattern only matches workspaces that are both:
1. Listed as dependencies of the current workspace
2. Match the glob pattern

This ensures that task dependencies follow the same dependency graph as your packages, preventing accidental coupling between unrelated workspaces.

## Parallel cross-workspace dependencies

Cross-workspace dependencies also support the parallel `&` modifier:

```
# In packages/app/taskfile
build: pkg-utils:build& pkg-core:build&
  echo "Building app"
```

Now `pkg-utils:build` and `pkg-core:build` run in parallel before `app:build`.

You can mix local and cross-workspace dependencies with any combination of sequential and parallel:

```
build: setup pkg-utils:build& pkg-core:build& finalize
  echo "Building app"
```

Execution order:
1. `setup` runs first
2. `pkg-utils:build` and `pkg-core:build` run in parallel
3. `finalize` runs after both complete
4. `build` runs last

## Transitive dependencies

Yarn automatically resolves transitive dependencies. If task A depends on B, and B depends on C, running A will execute C, then B, then A:

```
# packages/pkg-a/taskfile
build:
  echo "Building pkg-a"

# packages/pkg-b/taskfile
build: pkg-a:build
  echo "Building pkg-b"

# packages/pkg-c/taskfile
build: pkg-b:build
  echo "Building pkg-c"
```

Running `yarn build` in `pkg-c` will:
1. Execute `pkg-a:build` first (no dependencies)
2. Execute `pkg-b:build` after `pkg-a` completes
3. Execute `pkg-c:build` after `pkg-b` completes

The dependency resolution computes the full transitive closure, so `pkg-c:build` knows it must wait for both `pkg-a:build` and `pkg-b:build`.

## Running a task across workspaces

When a cross-workspace dependency uses a glob matching every dependency, you can write it with the `^` shorthand: `^build` is equivalent to `*:build`, meaning "the `build` task of every workspace listed in my `dependencies` or `devDependencies`". Workspaces that don't declare the task are skipped.

```
# In packages/app/taskfile
build: ^build
  tsc -b
```

## Root defaults

Declaring the same task in every workspace quickly becomes repetitive. Tasks tagged with the `@workspaces` attribute in the root workspace's `taskfile` act as defaults for every other workspace of the project:

```
# taskfile (at the root of the project)
@workspaces
build: ^build

@workspaces
test:ci: ^build ^test:ci

@workspaces
check: test:ci

@workspaces
@long-lived
dev: ^build
```

- A default task without a script body runs the `package.json` script of the same name (`yarn run <name>`, forwarding the extra arguments) in each workspace that defines it.
- In a workspace without the matching script, the task is a no-op that still propagates ordering: if `app` depends on `lib` which depends on `utils`, and only `app` and `utils` have a `build` script, `utils:build` still runs before `app:build`.
- A workspace can override a default by declaring a task with the same name in its own `taskfile`.
- Defaults apply to every workspace except the root workspace itself, which only runs the tasks it declares without the attribute.

Running `yarn run build` (or `yarn build`) still runs the `package.json` script directly; use `yarn tasks run` to go through the task graph.

## Selecting workspaces

By default `yarn tasks run <name>` runs the task (and its dependencies) in the current workspace. The following options select other workspaces; the selected tasks are resolved into a single graph in which each task runs at most once:

| Option | Selection |
| --- | --- |
| `-A,--all` | Every workspace declaring the task |
| `--from <glob>` | Workspaces matching an ident glob (`@scope/*`) or a path glob (`packages/*` or `./packages/*`); repeatable |
| `--affected` | Workspaces changed since the base refs (`changesetBaseRefs`), and all their dependents |
| `--since <ref>` | Same as `--affected`, against an explicit ref |
| `--with-dependencies` | Adds the workspace dependencies of the selection (alias: `--recursive`) |
| `--dependencies-only` | Replaces the selection by its workspace dependencies |
| `--with-dependents` | Adds the workspaces depending on the selection |
| `--include <glob>` / `--exclude <glob>` | Filter the final selection |

When `--with-dependents` and `--with-dependencies` are combined, dependencies are followed from the dependents too, so the selection contains everything needed to build each dependent.

Options must be placed before the task name (everything after it is forwarded to the task). Several tasks can be run together by separating them with commas:

```bash
yarn tasks run -A build,typecheck
yarn tasks run --from my-app --dependencies-only build
yarn tasks run --affected check
```

Other options:

- `--only` ignores cross-workspace dependencies pointing outside of the selection (for example to only build the selected workspaces without their dependencies).
- `-j,--concurrency <n>` limits the number of processes running at the same time (defaults to the number of CPUs when running across workspaces).
- `--continue` keeps running the independent tasks after a failure; by default the first failure cancels the run when running across workspaces. The exit code is the one of the first failing task.
- `--errors-only` only prints the output of the tasks that failed, followed by a summary.
- `--standalone` runs the tasks in an in-process daemon rather than the background one. It's enabled by default on CI (when the `CI` environment variable is set); use `--no-standalone` to opt out.

## Long-lived tasks

Tasks marked with `@long-lived` (dev servers, watchers) unblock their dependents after a warm-up period rather than when they exit. Combined with `^build`, `yarn tasks run --from my-app dev` first builds the dependencies of `my-app`, then starts its dev server and keeps it running. When connected to the background daemon, pressing Ctrl-C detaches from the server (`yarn tasks stop dev` stops it); with `--standalone`, Ctrl-C and SIGTERM stop it.

## Including tasks from other workspaces

You can include task definitions from dependency workspaces using the `include` directive. This allows you to reuse common task definitions across multiple workspaces without duplicating them.

### Basic include

To include all tasks from a dependency workspace's taskfile:

```
include pkg-utils

build: lint
  echo "Building"
```

This imports all tasks defined in `pkg-utils`'s `taskfile` into the current workspace. If `pkg-utils` has a `lint` task, it becomes available as if it were defined locally.

### Include with custom path

By default, `include` loads the `taskfile` at the root of the target workspace. You can specify a custom path:

```
include pkg-utils/tasks/common.tasks

build: lint typecheck
  echo "Building"
```

This loads tasks from `tasks/common.tasks` within the `pkg-utils` workspace instead of the default `taskfile`.

### Scoped package includes

Scoped packages are fully supported:

```
include @my-scope/my-lib

build: lint
  echo "Building"
```

You can also specify a custom path for scoped packages:

```
include @my-scope/my-lib/tasks/shared.tasks
```

### Include requirements

The include target must be declared as a dependency (or devDependency) in your `package.json`. This ensures that task includes follow the same dependency graph as your packages:

```json
{
  "name": "my-app",
  "dependencies": {
    "pkg-utils": "workspace:*"
  }
}
```

If you try to include a workspace that isn't a dependency, you'll get an error:

```
Error: Cannot include 'pkg-other' from 'my-app': not listed as a dependency
```

### Task precedence

When including tasks, local task definitions take precedence over included ones. If both your taskfile and an included taskfile define the same task name, your local definition is used:

```
include pkg-utils

# This overrides any 'build' task from pkg-utils
build:
  echo "Custom build"
```

### Multiple includes

You can include from multiple workspaces:

```
include pkg-utils
include pkg-core

build: lint typecheck test
  echo "Building"
```

Tasks from earlier includes take precedence over later ones if there are naming conflicts.

## Cycle detection

Yarn detects circular dependencies and reports an error:

```
# This will fail!
a: b
  echo "A"

b: c
  echo "B"

c: a
  echo "C"
```

Running any of these tasks will result in an error indicating the cycle: `a -> b -> c -> a`.

## Caching

Tasks can opt into result caching with the `@cache` attribute. When a cached task is about to run, Yarn computes a fingerprint of everything that may influence its result. If a previous successful run had the same fingerprint, Yarn restores the files it produced, replays its output, and skips the script entirely:

```
@cache
@inputs(src/** tsconfig.json)
@outputs(dist/**)
@env(NODE_ENV API_URL)
build: pkg-utils:build
  tsc -p tsconfig.build.json
```

```
$ yarn build
[my-app:build]: Cache hit, replaying output
...
```

The cache hit marker is printed on stderr (or inline with `-v`), so the standard output of a task is identical whether it ran or got restored.

### Attributes

| Attribute | Description |
| --- | --- |
| `@cache` | Enables caching for the task. Required by the other attributes. |
| `@inputs(globs...)` | Files (relative to the workspace) whose content is part of the fingerprint. Defaults to all the files of the workspace not ignored by git. |
| `@outputs(globs...)` | Files (relative to the workspace) stored after a successful run and restored on cache hits. Defaults to nothing (the task is only cached for its output and to skip work). |
| `@env(names...)` | Environment variables whose values are part of the fingerprint. A trailing `*` matches all the variables with that prefix (`NEXT_PUBLIC_*`). |

Values are space-separated lists; attributes can be repeated, in which case the lists are concatenated. In `@inputs`, the `@default` token stands for the default input set, so `@inputs(@default ../../specs/**)` adds files from outside the workspace without having to list its own files (like Turborepo's `$TURBO_DEFAULT$`); exclusions apply to it as well. Patterns starting with `!` exclude files, and a pattern matching a folder matches its whole content (`dist` is equivalent to `dist/**`). `@inputs()` declares a task without file inputs.

Long-lived tasks (`@long-lived`) cannot be cached.

### What's in the fingerprint

The fingerprint of a task covers:

- the task name, workspace, script, and arguments;
- the content and executable bit of its input files. When `@inputs` is omitted, all the files of the workspace are used, except those ignored by git (`.gitignore`, `.git/info/exclude`, and the global excludes file, when inside a git repository), the declared outputs, the `taskfile`, nested workspaces, and the `node_modules` and `.yarn` folders. The workspace `package.json` is always an input;
- the fingerprints of all the tasks it depends on, cached or not. Changing anything in `pkg-utils:build` (its inputs, script, or dependencies) thus invalidates `my-app:build`;
- the hash of the workspace's resolved dependency tree, as described by the lockfile. Upgrading a dependency of a workspace (directly or transitively) invalidates its tasks, but not those of the workspaces whose dependency tree didn't change;
- the values of the declared environment variables (unset and empty are different);
- the platform (OS and architecture), the Node.js version, and the Yarn version;
- the content of the files matching the `taskCacheGlobalInputs` setting, which is useful for repository-wide files such as `.nvmrc`:

```yaml
taskCacheGlobalInputs:
  - .nvmrc
  - patches/**
```

Undeclared environment variables are **not** part of the fingerprint; declare everything your task reads that may change its outputs.

Use `yarn tasks hash <task>` to print the fingerprint of a task along with everything that went into it (environment variable values are printed as hashes). Add `--json` and diff two outputs to find out why a task wasn't restored.

### Outputs

On a cache hit, Yarn first removes the files currently matching the `@outputs` globs (so stale files from previous runs don't linger), then restores the stored ones with their permissions. Symbolic links are preserved as such. If the files on disk already match the stored ones, nothing is written.

Failed runs (non-zero exit code) are never cached. Successful runs whose output globs match nothing are cached nonetheless, which is useful for checks like linting.

### Managing the cache

- `yarn tasks run --no-cache <task>` (alias `--force`) ignores the existing entries; the results of successful runs are still stored.
- `yarn tasks cache clean` removes all the entries.
- The `enableTaskCache` setting (default `true`) disables caching entirely when set to `false`.
- The `taskCacheFolder` setting (default `.yarn/ignore/task-cache`) controls where the entries are stored. Each entry is a zip archive named after its fingerprint, written atomically, so several processes can safely share the same folder.

Yarn also memoizes the hashes of the files it reads (keyed by their path, size, modification time, and inode) in `.yarn/ignore/task-cache-files`, so that checking an unchanged task only requires a `stat` call per input file.
