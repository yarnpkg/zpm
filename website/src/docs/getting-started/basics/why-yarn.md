---
category: getting-started
slug: welcome
title: Why Yarn?
description: What makes Yarn different, and why it might be the right package manager for your project.
sidebar:
  order: 1
---

Yarn is an open-source package manager for JavaScript and TypeScript projects. It focuses on correctness, performance, and developer experience — and ships with the tools you'd otherwise need to assemble yourself.

## Correctness by default

Most package managers use a flat `node_modules` layout that lets your code import packages you never declared as dependencies. These *ghost dependencies* cause builds that work today and break tomorrow, when the hoisting layout changes.

Yarn's default install strategy, [Plug'n'Play](/concepts/pnp), eliminates this class of bugs entirely. It replaces `node_modules` with a Node.js loader that enforces your declared dependency tree, so every import is validated at runtime. PnP is supported natively by Vite, Webpack, Esbuild, Rspack, ESLint, and many more.

## Fast and efficient

PnP installs avoid copying files into `node_modules` altogether. Dependencies are stored in a content-addressable cache and resolved directly, bringing typical install times down to a few seconds — even on large projects.

If a few seconds is still too many, [zero-installs](/concepts/zero-installs) lets you check your install artifacts into version control so that `git clone` is all you need. No install step, no waiting.

Yarn tracks comparative benchmarks continuously on every commit, and publishes them on a [public dashboard](/concepts/performances).

## Batteries included

Yarn ships features that other package managers leave to third-party tools:

- [**Workspaces**](/concepts/workspaces) are a core part of Yarn's design. Every project is a workspace, and monorepos get first-class support for cross-package references, shared dependency ranges via the `catalog:` protocol, and more.

- [**Constraints**](/concepts/constraints) let you lint and auto-fix your entire dependency tree — enforce version consistency, detect circular dependencies, or validate `package.json` structure across all your workspaces.

- [**Task dependencies**](/concepts/tasks) give you a built-in task runner with sequential and parallel execution, cross-workspace dependencies, and glob patterns — no need for a separate orchestration tool.

- [**Dependency patching**](/concepts/patches) lets you fix a bug in a dependency without forking the repository.

- [**Node.js management**](/concepts/nvm) treats Node.js as a project dependency via `@yarnpkg/node`. The version is locked in `yarn.lock`, so every team member and CI runner uses exactly the same one.

- [**Workspace profiles**](/concepts/profiles) let you declare shared dev dependencies once and apply them to any workspace, keeping your monorepo configuration DRY.

## Flexible installation strategies

Not every project can adopt PnP on day one. Yarn supports [three linker modes](/concepts/node-linkers), all stable and production-ready:

- **PnP** (default) — the fastest and strictest option, with content-addressable storage and ghost dependency protection.
- **pnpm mode** — a symlink-based layout that offers a good middle ground between speed and compatibility.
- **node-modules mode** — a traditional `node_modules` tree for maximum ecosystem compatibility.

You can start with `node-modules` and migrate to PnP when you're ready. The switch is a single configuration change.

## Independent and open-source

Yarn isn't proprietary, and it isn't backed by venture capital. It's maintained by an independent team, and its roadmap is driven by community needs rather than commercial strategy.

## How is Yarn different from X?

Every package manager makes different trade-offs. For a detailed look at how Yarn compares to npm, pnpm, and bun, see our [comparisons page](/getting-started/comparisons).
