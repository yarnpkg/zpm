---
category: getting-started
slug: getting-started/migrating/turborepo
title: Migrating from Turborepo
description: How to express a turbo.json pipeline with Yarn tasks, and the Yarn equivalents of the common turbo commands.
---

Yarn tasks can replace the task orchestration part of [Turborepo](https://turborepo.com): running `package.json` scripts across workspaces in dependency order, filtering workspaces, and detecting the workspaces affected by a change. This page maps `turbo.json` and the turbo CLI to their Yarn equivalents.

:::note
Yarn doesn't cache task results at the moment; tasks always run. `outputs`, `inputs`, `env`, and `cache` from `turbo.json` have no equivalent yet.
:::

## Converting turbo.json

Turbo task definitions become `@workspaces` tasks in the `taskfile` at the root of the project. A task without a body runs the `package.json` script of the same name in each workspace (and is a no-op that still propagates ordering in workspaces without that script, like in turbo).

```json
{
  "globalDependencies": [".nvmrc", "patches/**"],
  "tasks": {
    "build": {"dependsOn": ["^build"], "outputs": ["dist/**"]},
    "test:ci": {"dependsOn": ["^build", "^test:ci"]},
    "check": {"dependsOn": ["test:ci"]},
    "check-and-build": {"dependsOn": ["build", "test:ci"]},
    "typecheck": {},
    "clean": {"cache": false},
    "dev": {"dependsOn": ["^build"], "persistent": true, "cache": false},
    "start": {"dependsOn": ["build"], "persistent": true}
  }
}
```

becomes:

```
# taskfile
@workspaces
build: ^build

@workspaces
test:ci: ^build& ^test:ci&

@workspaces
check: test:ci

@workspaces
check-and-build: build& test:ci&

@workspaces
typecheck:

@workspaces
clean:

@workspaces
@long-lived
dev: ^build

@workspaces
@long-lived
start: build
```

and the global dependencies go in `.yarnrc.yml`:

```yaml
changesetGlobalFiles:
  - .nvmrc
  - patches/**
```

| turbo.json | taskfile |
| --- | --- |
| `"dependsOn": ["^build"]` | `build: ^build` |
| `"dependsOn": ["lint"]` | `build: lint` |
| `"dependsOn": ["a", "b"]` (unordered) | `task: a& b&` |
| `"dependsOn": ["pkg#build"]` | `task: pkg:build` (`pkg` must be a dependency of the workspace) |
| `"persistent": true` | `@long-lived` |
| per-package `turbo.json` | a `taskfile` in the workspace overriding the default |
| `globalDependencies` | `changesetGlobalFiles` setting |

Dependencies without `&` are sequential (`a b` runs `a` then `b`); add `&` to run them in parallel, which matches turbo's unordered `dependsOn`.

## Commands

Options to `yarn tasks run` must be placed before the task name.

| turbo | Yarn |
| --- | --- |
| `turbo run build` | `yarn tasks run -A build` |
| `turbo run build lint` | `yarn tasks run -A build,lint` |
| `turbo build --filter pkg` | `yarn tasks run --from pkg build` |
| `turbo build --filter pkg...` | `yarn tasks run --from pkg --with-dependencies build` |
| `turbo build --filter pkg^...` | `yarn tasks run --from pkg --dependencies-only build` |
| `turbo build --filter ...pkg` | `yarn tasks run --from pkg --with-dependents build` |
| `turbo build --filter './packages/*'` | `yarn tasks run --from './packages/*' build` |
| `turbo build --filter '!pkg'` | `yarn tasks run -A --exclude pkg build` |
| `turbo build --only --filter a --filter b` | `yarn tasks run --only --from a --from b build` |
| `turbo run check --affected` | `yarn tasks run --affected check` |
| `turbo build --concurrency 4` | `yarn tasks run -A -j 4 build` |
| `turbo build --continue` | `yarn tasks run -A --continue build` |
| `turbo build --output-logs=errors-only` | `yarn tasks run -A --errors-only build` |
| `turbo run dev --filter app` | `yarn tasks run --from app dev` |
| `turbo ls --affected --output json` | `yarn workspaces list --since -R --json` |

### Affected workspaces

`yarn workspaces list --since -R --json` prints one JSON object per line (`{"location": "packages/app", "name": "app"}`) for each workspace that changed since the merge base with `changesetBaseRefs` (`main` and `master` by default), plus all the workspaces depending on them. A workspace is changed when:

- a file inside it changed (including uncommitted and untracked files),
- the lockfile changed in a way that affects its dependency tree,
- a file matching `changesetGlobalFiles` changed (every workspace is then affected).

Use `--since <ref>` to compare against a specific ref, and `--head <ref>` to compare two refs rather than the working tree. The `TURBO_SCM_BASE` and `TURBO_SCM_HEAD` environment variables (or `YARN_CHANGESET_BASE` and `YARN_CHANGESET_HEAD`) are honored when no explicit ref is given.

## CI

On CI (when the `CI` environment variable is set), `yarn tasks run` runs the tasks in an in-process daemon instead of the background one, so nothing outlives the job. A failure cancels the remaining tasks unless `--continue` is set, and the command exits with the exit code of the first failing task. SIGTERM stops all running tasks.
