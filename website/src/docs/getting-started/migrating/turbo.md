---
category: getting-started
slug: getting-started/migrating-from-turbo
title: Migrating from Turbo
description: Translate Turborepo task definitions into Yarn taskfiles, including workspace defaults, dependencies, and local overrides.
---

Yarn's [task runner](/concepts/tasks) can express shared workspace tasks and their execution order in a root `taskfile`. This guide covers migrating those parts of a Turborepo setup. Task artifact caching and other compatibility differences are described below.

## Move shared task definitions into a taskfile

Consider this `turbo.json`:

```json
{
  "tasks": {
    "build": {
      "dependsOn": ["^build"]
    },
    "test": {
      "dependsOn": ["build"]
    }
  }
}
```

Create a `taskfile` next to the root `package.json`:

```text
@defaultTask
build: *:build&

@defaultTask
test: build
```

Keep the `build` and `test` scripts in each workspace's `package.json`. The `@defaultTask` annotation supplies the task to every non-root workspace that doesn't define its own task with the same name. When the default has no commands, Yarn runs the workspace's matching package script after its dependencies complete.

The `&` suffix allows dependency builds to run concurrently. Yarn dependencies are sequential unless marked with `&`; Turbo's `dependsOn` array doesn't impose an order between its entries.

## Translate task dependencies

| Turbo configuration | Yarn taskfile |
| --- | --- |
| `"dependsOn": ["build"]` | `test: build` |
| `"dependsOn": ["^build"]` | `build: *:build&` |
| `"dependsOn": ["lint", "build"]` | `test: lint& build&` |
| `"dependsOn": ["utils#build"]` | `build: utils:build` |

In Yarn, `utils:build` and `*:build` only match workspaces declared in the current workspace's `dependencies` or `devDependencies`. Turbo's explicit `package#task` references can point outside those dependency relationships, so check those references during migration.

Yarn uses `*:build` for dependency workspace builds. The `^build` syntax belongs in `turbo.json` and isn't accepted in a taskfile.

## Run a task

To run an application's test task and everything it depends on, select that workspace:

```bash
yarn ./packages/app tasks run test
```

Use `yarn tasks run <name>` when you want task orchestration. `yarn run <name>` executes a matching package script directly, so `yarn run test` won't first build dependencies when the workspace has its own `test` script.

The root `@defaultTask` declarations don't create root tasks or start every workspace's task. Unlike `turbo run build` from the root, `yarn tasks run build` targets the current workspace. To invoke it in every child workspace, use:

```bash
yarn workspaces foreach -A --exclude my-monorepo tasks run build
```

Replace `my-monorepo` with the root package's name. Each invocation has its own task graph, so shared dependencies may build more than once across these invocations. Targeting an application's task runs its dependency graph within a single invocation.

For CI, add `--standalone` before the task name to run without connecting to a background daemon:

```bash
yarn ./packages/app tasks run --standalone test
```

## Preserve ordering through workspaces without scripts

An inherited task without a matching package script still waits for its task dependencies. This is similar to Turbo's [transit nodes](https://turborepo.com/docs/core-concepts/package-and-task-graph#transit-nodes).

For example, suppose `app` depends on `bridge`, which depends on `core`. Only `app` and `core` have a `build` script. With the shared `build: *:build&` definition above, running the build task in `app` produces this order:

```text
core:build   → runs core's build script
bridge:build → completes without running a command
app:build    → runs app's build script
```

There is no need to add an empty build script to `bridge`. Its inherited task keeps `app` waiting for `core`.

## Override a task in one workspace

A workspace can override a root default by defining the task in its own taskfile:

```text
# packages/app/taskfile
build: *:build&
  yarn run build "$@"
  yarn run generate-manifest
```

The local definition replaces the whole default, including dependencies and commands. Repeat the dependency list you want to keep. An empty local definition doesn't fall back to the package script; automatic script lookup only applies to inherited defaults without commands.

For a task that only belongs to the root workspace, use an ordinary, unannotated task there. Choose a separate name if the root taskfile already supplies a default with that name.

## Handle script names containing colons

Yarn reserves colons in task references for `workspace:task`. Task names themselves cannot contain colons. Keep a package script such as `test:ci` and give its task a name such as `test-ci`:

```text
@defaultTask
test-ci: build
  yarn run test:ci "$@"
```

This explicit command runs in every workspace inheriting the default, so each must provide the `test:ci` script. Alternatively, rename the package scripts to `test-ci` and omit the command body to use the automatic script lookup.

## Review features beyond task ordering

Yarn's task runner currently has no equivalent for Turbo's task artifact cache, remote cache, or `inputs` / `outputs` hashing. Yarn's package download cache serves a different purpose. Keep Turbo for workflows that require its task caching until you have a replacement for that behavior.

Other Turbo configuration and flags, such as environment filtering, `--affected`, `with`, and `persistent`, aren't automatically translated by these examples. Review them separately before removing Turbo. In particular, Yarn's `@long-lived` tasks have their own daemon lifecycle and aren't a direct translation of `persistent: true`.

Update CI and root scripts that invoke `turbo run`, then verify dependency order and generated files with the new commands. Once all required workflows have migrated, remove the unused Turbo configuration and dependency.
