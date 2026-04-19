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
