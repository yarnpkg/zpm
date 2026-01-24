---
category: concepts
slug: concepts/node-linkers
title: Node.js linkers
description: The different ways to install your project.
sidebar:
  order: 4
---

Yarn supports three different ways to install your projects on disk. This document gives a quick overview of each of them, along with the pros and cons of each.

:::note
All install modes are **stable** and **production-ready**. Yarn uses PnP installs by default, but the `pnpm` and `node-modules` linkers are first-class citizens as well, supported by a wide range of tests.
:::

## `nodeLinker: pnp`

*For more details about Plug'n'Play installs, check the [dedicated section](/concepts/pnp).*

Under this mode Yarn will generate a single Node.js loader file directory referencing your packages from their cache location. No need for file copies, or even symlinks / hardlinks.

<div class="[&_table]:table-fixed [&_th]:w-[50%]">

| Pros | Cons |
| --- | --- |
| Extremely fast | Less idiomatic |
| Content-addressable store | IDE integrations often require [SDKs](/getting-started/editor-sdks) |
| Protects against ghost dependencies | Sometimes requires `packageExtensions` |
| Semantic dependency errors | |
| Perfect hoisting optimizations | |
| Provides a [dependency tree API](/advanced/pnpapi) | |
| Can be upgraded into [zero-installs](/features/caching#zero-installs) | |

</div>

:::note
Yarn Plug'n'Play has been the default installation strategy in Yarn since 2019, and the compatibility story significantly improved along the years as we worked with tooling authors to smoothen the edges.
:::

## `nodeLinker: pnpm`

Under this mode, a flat folder is generated in `node_modules/.pnpm` containing one folder for each dependency in the project. Each dependency folder is populated with hardlinks obtained from a central store common to all projects on the system (by default `$HOME/.yarn/berry/index`). Finally, symlinks to the relevant folders from the flat store are placed into the `node_modules` folders.

<div class="[&_table]:table-fixed [&_th]:w-[50%]">

| Pros | Cons |
| --- | --- |
| Slower than PnP, but still very fast | Symlinks aren't always supported by tools |
| Content-addressable store | Hard links can lead to strange behaviors |
| Protects against _some_ ghost dependencies | Generic dependency errors |
| No need for IDE SDKs | Sometimes requires `packageExtensions` |

</div>

:::note
The pnpm mode is an interesting middle ground between traditional `node_modules` installs and the more modern Yarn PnP installs; it doesn't decrease the performances much and provides a slightly better compatibility story, at the cost of losing a couple of interesting features.
:::

## `nodeLinker: node-modules`

This mode is the old tried and true way to install Node.js projects, supported natively by Node.js and virtually the entirety of the JavaScript ecosystem.

While we tend to recommend trying one of the two other modes first, it remains a solid option in case you face problems with your dependencies that you don't have the time to address right now. Sure, your project may be a little more unstable as you won't notice if ghost dependencies creep in, but it may be a reasonable trade-off depending on the circumstances.

<div class="[&_table]:table-fixed [&_th]:w-[50%]">

| Pros | Cons |
| --- | --- |
| Perfect compatibility with the whole ecosystem | Average speed |
| Optional support for hardlinks (`nmMode`) | No protection against ghost dependencies |
| No need for IDE SDKs | Imperfect hoisting due to the filesystem reliance |

</div>
