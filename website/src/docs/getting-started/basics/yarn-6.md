---
category: getting-started
slug: concepts/yarn-6
title: Yarn 6.x
description: What's new in Yarn 6, the Rust-based rewrite of Yarn.
sidebar:
  order: 2
---

Yarn 6.x is a ground-up rewrite of Yarn in Rust, designed to push past the performance ceiling that JavaScript imposed on large-scale monorepos. While the core principles remain the same - correctness, developer experience, and performance - the native implementation unlocks dramatically faster operations and lower memory footprints.

:::note
Yarn 6.x is currently in preview. The first stable release is expected in **Q3 2026**. Preview releases are already deployed in production at Datadog with minimal breaking changes.
:::

## Performance

The Rust rewrite delivers significant speed improvements across the board, particularly on warm cache scenarios where Yarn 6.x is up to 5x faster than its JavaScript predecessor:

| Project | Cache | Yarn 4.x | Yarn 6.x |
| --- | --- | --- | --- |
| Next.js | Cold | 4.1s | 2.5s |
| Next.js | Warm | 577ms | 184ms |
| Gatsby | Cold | 19.8s | 11.7s |
| Gatsby | Warm | 1.7s | 0.3s |

These gains are especially impactful in massive monorepos, where install times were previously a bottleneck. They also enable features that would have been too expensive to run in JavaScript, such as Lazy Installs.

## Lazy Installs

Previous Yarn versions offered two modes: regular installs (run `yarn install` after every pull) and [Zero Installs](/features/caching#zero-installs) (check install artifacts into the repository). Zero Installs removed the need for explicit installs but came at the cost of repository size, which became prohibitive in large monorepos.

Yarn 6.x introduces a third option as the new default: **Lazy Installs**.

Under this model, Yarn silently performs an install whenever it detects that the on-disk artifacts are out of sync with `package.json`. This check happens automatically before most commands - including `yarn run` - and has negligible overhead in the happy path thanks to the native Rust implementation.

In practice, this means you no longer need to remember to run `yarn install` after switching branches or pulling changes. Yarn handles it for you, without bloating your repository with cached packages.

## Yarn Switch

With Node.js [phasing out Corepack](https://github.com/nodejs/TSC/pull/1697#issuecomment-2737093616), Yarn 6.x ships with its own version manager: [Yarn Switch](/concepts/switch). Written in Rust, it reads the `packageManager` field from your project and transparently downloads, caches, and forwards commands to the correct Yarn version.

For more details, see the [Yarn Switch documentation](/concepts/switch).

## Versioning roadmap

The transition from Yarn 4.x to 6.x follows a deliberate path:

1. **Yarn 5.x** is a stepping stone release based on the JavaScript codebase. It introduces some of the deprecations coming in 6.x, giving you time to adapt.
2. **Yarn 6.x** is the Rust-based release. Once stable, all active development will shift to this codebase.
3. **Yarn 5.x LTS** will receive critical bugfixes for approximately 30 months after 6.x reaches stable.

## Backward compatibility

Backward compatibility is a primary concern. Yarn 6.x is validated against the exact same test suite as its predecessors, ensuring that existing projects can upgrade with minimal friction.

Projects using the `pnp`, `pnpm`, or `node-modules` linkers will continue to work as before. The same applies to workspaces, constraints, and other Yarn features. If your project works on Yarn 4.x, it should work on Yarn 6.x.
