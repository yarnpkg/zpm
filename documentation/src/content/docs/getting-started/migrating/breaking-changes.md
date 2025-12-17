---
category: getting-started
slug: getting-started/breaking-changes
title: Breaking Changes
description: A detailed explanation of the breaking changes between two versions.
---

## Yarn 4 → Yarn 6

:::caution
This document lists the **intended breaking changes**. Yarn 6 being still in development, some features are still missing and will be implemented before we publish the first stable release.
:::

### Plugins

A special note for plugins, which aren't implemented **yet**.

Various other projects (Biome, Oxc, etc) are experimenting on that topic, and we prefer to wait before focusing on that so we can leverage their researches before building our own solutions.

### Lockfile

- The lockfile (`yarn.lock`) is now formatted in JSON to benefit from heavily optimized JSON parsers. Some of its layout has slightly changed:

  - All records are wrapped in an `entries` field.
  - Record definition have most of their fields wrapped in a `resolution` field.
  - We generally recommend using `yarn info --json` rather than manually parsing the lockfile.

- Workspaces aren't stored in the lockfile anymore as they would waste gigabytes of storage on very large monorepos despite Yarn never using those entries.

### Features

- Support for the legacy Prolog constraints engine has been dropped. Constraints must be migrated to the [JavaScript engine](/concepts/constraints) introduced in Yarn 4.

- Support for the `yarnPath` field has been dropped. Use [Yarn Switch](/concepts/switch) to manage Yarn versions in your repository. Use `yarn switch link` should you need to use a local binary.

- Support for the `--cwd` flag has been dropped. Instead, pass the cwd path as first argument on the CLI (for example `yarn ./packages/foo add lodash`, or `yarn /path/to/project install`). As long as it contains a slash, it'll be interpreted as a path (this syntax works with both Yarn Berry and Yarn ZPM).

### Internal design

- Yarn doesn't support anymore having multiple workspaces in the same project sharing the same name but with different version. If set, workspace names must be unique across the project.

- Yarn will now prioritize referencing workspaces by their name rather than their path when serializing their locators (ie you'll see `foo@workspace:foo` rather than `foo@workspace:packages/foo`).

- Yarn won't overwrite your `package.json` formatting anymore. This currently includes the sorting of the keys in the `dependencies` / `devDependencies` / `peerDependencies` fields.

- Yarn will automatically run transparent installs when it detects your project changed since the last time an install was run.

- The `yarn config` command, when called with no arguments, has a different output.

### Deprecations

- The `.pnp.cjs` file isn't generated with the `+x` flag anymore.
