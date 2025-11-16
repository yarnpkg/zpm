---
category: getting-started
slug: getting-started/migration-mode
title: Migration Mode
description: An advanced mode for gradually migrate large-scale monorepos to Yarn.
---

Migrating to new major releases of Yarn on small repositories is easy thanks to the `packageManager` field. As soon as you update it, either manually or through `yarn set version`, [Yarn Switch](/concepts/switch) will start using this new version. Committing the update in the repository will make sure everyone pulling your repository will use the exact version of Yarn you intended.

This workflow works well in the vast majority of cases - but what of large high-velocity monorepos? Imagine a monorepo receiving hundreds of PRs a day from dozens of contributors, with just as many workflows running on CIs. Performing upgrades between major releases there can be scary for developer experience teams - could the new version include an unforeseen regression that would impact your users?

The migration mode is a tool that lets you configure the repository so that only some people use the new release, allowing you to efficiently perform gradual rollouts.

:::caution
Keep in mind the procedure described here is intended for **high-velocity repositories** with **dozens of contributors**, which tend to have dedicated developer experience teams. For most other situations the `yarn set version` flow is recommended.
:::

## What is the migration mode?

When the migration mode is enabled (more on that later), [Yarn Switch](/concepts/switch) will read the package manager version from `packageManagerMigration` rather than `packageManager`. Yarn will also change some of its internal settings:

- The lockfile will be written in the `.yarn/ignore` folder rather than at the root of your repository.

- The local cache will be disabled; new downloaded packages will be stored in the global system cache.

- For Yarn PnP installs, the `.pnp.cjs` and `.pnp.loader.mjs` files are generated in the `.yarn/ignore` folder rather than at the root of the repository.

Those changes are all in the service of one goal: **the Yarn version you're migrating to isn't allowed to have lasting effect on the repository**. This ensure that only contributors who opted-in to the migration can be impacted by potential regressions.

## How to enable the migration mode?

1. Add a new `packageManagerMigration` field next to the existing `packageManager` field.
2. Anyone who wish to opt-in to the migration should run `yarn switch link --migration`.
3. Opting-out from the migration is as simple as running `yarn switch unlink`.

Yarn Switch will automatically unlink migrations once the `packageManagerMigration` field is removed from the repository.

## Special considerations

### Manipulating dependencies during a migration

Should you change your project dependencies while under the effects of a migration, Yarn will upgrade the migrated lockfile but not the mainstream one. Your CI workflows will likely report errors due to the automatic enablement of the `--immutable` flag.

To fix this issue, run the following command locally and check-in the produced changes:

```
YARN_ENABLE_MIGRATION_MODE=0 yarn install
```
