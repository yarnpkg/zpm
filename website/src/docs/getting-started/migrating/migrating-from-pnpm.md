---
category: getting-started
slug: getting-started/migrating-from-pnpm
title: Migrating from pnpm
description: How to migrate a pnpm project to Yarn without upgrading its dependencies.
---

Yarn can install pnpm projects with minimal changes; this page lists the steps involved, with a focus on the one thing you most likely care about: **keeping the dependency versions you already use**.

## Translating the configuration

pnpm and Yarn share most concepts, but they don't always configure them the same way:

| pnpm | Yarn |
| --- | --- |
| `pnpm-workspace.yaml` → `packages` | `workspaces` field in the root `package.json` |
| `pnpm-workspace.yaml` → `catalog` / `catalogs` | `catalog` / `catalogs` in `.yarnrc.yml` (see the [`catalog:` protocol](/protocol/catalog)) |
| `overrides` | `resolutions` field in the root `package.json` |
| `patchedDependencies` | `patch:` entries in `resolutions` |
| `packageExtensions` | `packageExtensions` in `.yarnrc.yml` |
| `minimumReleaseAge` (minutes) | `npmMinimalAgeGate` in `.yarnrc.yml` |
| `node_modules/.pnpm` layout | `nodeLinker: pnpm` in `.yarnrc.yml` (see the [linkers](/concepts/node-linkers)) |

:::tip
pnpm overrides such as `foo@1.2.3: 1.2.4` only apply to the dependencies whose range intersects with `1.2.3`; use the `foo@intersects:1.2.3` selector in `resolutions` to get the same behavior.
:::

## Keeping the locked versions

Without a lockfile, Yarn resolves every range to the highest version available on the registry. On a large project this can easily upgrade a quarter of the dependency tree in one go, which is rarely what you want when switching package managers.

To avoid that, Yarn reads your `pnpm-lock.yaml`. When you run `yarn install` in a project that has a `pnpm-lock.yaml` but no `yarn.lock`, Yarn prints a notice and prefers the versions pnpm locked:

```
➤ · No yarn.lock found, but found pnpm-lock.yaml; Yarn will prefer the 5995 versions it locks
```

Once the `yarn.lock` file has been written it becomes the source of truth, and the pnpm lockfile is ignored from then on; you can delete it after checking the result.

If you already generated a `yarn.lock` (or your pnpm lockfile is stored elsewhere), run `yarn import pnpm` instead. It discards the current `yarn.lock` and resolves everything again from the pnpm lockfile:

```bash
yarn import pnpm
yarn import pnpm --lockfile ../old-checkout/pnpm-lock.yaml
```

### How versions are picked

For each dependency resolved through the npm registry, Yarn:

1. Lists the versions of that package locked in `pnpm-lock.yaml` that satisfy the range, as Yarn computes it (ie. **after** applying your `catalogs` and `resolutions`).
2. If there's only one, uses it.
3. If there are several, uses the one pnpm locked for that exact range; it finds out by looking at the ranges declared by the workspaces and by the packages that depend on it.
4. If there's none (the package isn't in the pnpm lockfile, or its range changed since), resolves the dependency as usual.

Because locked versions are checked against the ranges Yarn resolves, the import never overrides your `resolutions`: if you translated an override differently, or changed a range in the meantime, the new range wins.

Versions locked by pnpm are **exempted from `npmMinimalAgeGate`**. The age gate is meant to keep newly published versions out of your project until they had time to be vetted; versions already locked by pnpm are, by definition, already in your project. Enforcing the gate on them would make Yarn pick different versions than the ones you were using, which defeats the purpose of the import. Versions resolved normally (step 4) are still subject to the gate.

### What may still change

A few differences are expected, and are worth reviewing in the generated lockfile:

- **Peer dependencies:** with `autoInstallPeers`, pnpm installs missing peer dependencies automatically. Yarn doesn't; it reports them as missing peer dependencies instead, so the packages pnpm installed solely to fulfill them won't be in the `yarn.lock`. Add them to the relevant workspaces if you need them.
- **Duplicated ranges:** pnpm sometimes locks the same range to different versions in different places (typically when a lockfile got updated piecemeal). Yarn resolves each range only once, so it picks the version locked most often for that range.
- **Translated overrides and patches:** if your `resolutions` don't exactly match pnpm's overrides, the affected packages (and their dependencies) resolve differently.

Only `pnpm-lock.yaml` files using the lockfile format version 6 or 9 (pnpm 8 and later) can be imported.
