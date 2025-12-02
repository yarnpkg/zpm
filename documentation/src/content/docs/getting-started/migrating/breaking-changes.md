---
category: getting-started
slug: getting-started/breaking-changes
title: Breaking Changes
description: A detailed explanation of the breaking changes between two versions.
---

## Yarn 4 → Yarn 6

### Lockfile

- The lockfile (`yarn.lock`) is now formatted in JSON to benefit from heavily optimized JSON parsers. Some of its layout has slightly changed:

  - All records are wrapped in an `entries` field.
  - Record definition have most of their fields wrapped in a `resolution` field.
  - We generally recommend using `yarn info --json` rather than manually parsing the lockfile.

- Workspaces aren't stored in the lockfile anymore as they would waste gigabytes of storage on very large monorepos despite Yarn never using those entries.

### Features

- The Prolog constraints engine has been fully removed. Constraints must be migrated to the [JavaScript engine](/concepts/constraints).

- Support for the `yarnPath` field has been removed. Use [Yarn Switch](/concepts/switch) to manage Yarn versions in your repository. Use `yarn switch link` should you need to use a local binary.

### Internal design

- Yarn doesn't support anymore having multiple workspaces in the same project sharing the same name but with different version. If set, workspace names must be unique across the project.

- Yarn will now prioritize referencing workspaces by their name rather than their path when serializing their locators (ie you'll see `foo@workspace:foo` rather than `foo@workspace:packages/foo`).

- Yarn doesn't overwrite your `package.json` formatting anymore. This include the sorting of the keys in the `dependencies` field.
