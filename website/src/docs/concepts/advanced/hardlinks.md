---
category: concepts
slug: concepts/hardlinks
title: Hardlinks and the store
description: How Yarn shares files between projects, and what happens when it can't.
sidebar:
  order: 8
---

Some install modes don't write a separate copy of each package file into your project. Instead, they place [hard links](https://en.wikipedia.org/wiki/Hard_link) pointing to a single copy shared by every project on the machine. This page explains how that works, when Yarn has to fall back to regular copies, and how to avoid it.

## The store

When it installs a package, Yarn stores each of its files in a content-addressed store: a folder where files are named after the hash of their content, located under the `index` folder of the [global folder](/configuration/yarnrc#field-globalfolder) (`~/.yarn/zpm/index` by default). The files Yarn puts in your project are then hard links to the files of the store.

A hard link isn't a shortcut like a symlink: it's another name for the exact same file on disk. A file that appears in a hundred projects is stored only once, and linking it into a new project is almost free, as Yarn doesn't need to write its content again.

The store is used by the following [linkers](/concepts/node-linkers):

- `nodeLinker: pnpm`, which always links the files of its virtual store from the global store.
- `nodeLinker: node-modules` with `nmMode: hardlinks-global`.

With `nmMode: hardlinks-local`, Yarn doesn't use the global store; instead, identical files within the project are hard links to each other.

## When Yarn copies files instead

Hard links have two limitations, and when Yarn hits one of them it copies the file instead of linking it (with the same permissions, so executables stay executable). The install succeeds, but Yarn prints a warning at the end of the linking step:

```
➤ │ 1182 files were copied instead of hard-linked because the store at /root/.yarn/zpm/index and the project at /app are on different filesystems; installs are slower and use more disk space as a result (see https://yarnpkg.com/concepts/hardlinks)
```

### Different filesystems

A hard link can only point to a file on the same filesystem (or drive, on Windows). If the store and the project's `node_modules` folders live on different filesystems, no file can be linked. This is common in setups such as:

- **Docker containers** where the project is a bind mount (`docker run -v $(pwd):/app`), or where `node_modules` is a named volume, while the global folder lives in the container's own filesystem under `/root`.
- **CI environments** where the checkout lives on a different disk than the home folder; for example GitHub Actions Windows runners check out the repository on `D:` while the home folder is on `C:`.
- **Separate partitions**, for example when `/home` is its own partition and the project lives under `/srv` or `/data`, or when working from `/mnt/c` in WSL.
- **Windows drives**, when the project lives on another drive than your user profile.

Only the files that cross the filesystem boundary are affected; if just one workspace's `node_modules` is on another filesystem, only its files are copied.

### Link limit

Filesystems limit how many hard links a single file can have: 1023 on NTFS, 65000 on ext4. Since the store keeps a single copy of each content, very common files (empty files, identical license files, ...) can reach that limit when many projects share the same store. Yarn then copies the files that can't be linked anymore.

## What it costs

A copied file isn't shared anymore: each project gets its own copy, which takes more disk space, and installs are slower as Yarn has to write every file instead of creating a link.

Subsequent installs don't rewrite the copies: just like linked files, they are kept as long as they are up to date. If you modify a copied file inside `node_modules`, running `yarn install --force` restores it, as it would for a linked file.

## How to fix it

### Keep the global folder on the same filesystem as the project

The best fix is to move the global folder to the filesystem the project lives on, either by setting [`globalFolder`](/configuration/yarnrc#field-globalfolder) in your `.yarnrc.yml`:

```yaml
globalFolder: /data/.yarn-global
```

Or by setting the `YARN_GLOBAL_FOLDER` environment variable:

```bash
export YARN_GLOBAL_FOLDER=/data/.yarn-global
```

:::note
The global folder also contains the global cache and other metadata, so they will be downloaded again in the new location. If you put the global folder inside the project, don't forget to add it to your `.gitignore`.
:::

**Docker:** when the project is bind mounted, put the global folder in the mount as well:

```dockerfile
WORKDIR /app
ENV YARN_GLOBAL_FOLDER=/app/.yarn/global
```

If `node_modules` is a named volume, the store can't be on the same filesystem as both the volume and the rest of the project; prefer one of the [modes that don't use the store](#use-a-mode-that-doesnt-use-the-store), or stop mounting `node_modules` separately.

**CI:** point the global folder at a folder on the same disk as the checkout, and cache it from there. For example, on GitHub Actions the runner's temporary folder is on the same drive as the workspace:

```yaml
env:
  YARN_GLOBAL_FOLDER: ${{ runner.temp }}/yarn-global

steps:
  - uses: actions/checkout@de0fac2e4500dabe0009e67214ff5f5447ce83dd # 6.0.2

  - uses: actions/cache@v4
    with:
      path: ${{ runner.temp }}/yarn-global
      key: yarn-${{ runner.os }}-${{ hashFiles('yarn.lock') }}
```

Once the setup is fixed, run `yarn install --force` once to turn the existing copies back into links.

If a copy can't be replaced (on Windows, typically because a process has the file open), Yarn keeps it, reports it in the warning, and tries again on the next `yarn install --force`.

### Use a mode that doesn't use the store

If you can't move the global folder, you can switch to an install mode that doesn't rely on the store:

- `nmMode: hardlinks-local` (with `nodeLinker: node-modules`) only links identical files within the project, so it only needs the project's `node_modules` folders to be on the same filesystem.
- `nmMode: classic` (the default for `nodeLinker: node-modules`) writes regular files, without any hard link.
- [`nodeLinker: pnp`](/concepts/pnp) (the default) doesn't copy or link package files at all; most packages are read directly from the cache archives.

### Silence the warning

If the copies are fine for your use case (for example in a short-lived container), you can hide the warning with [`logFilters`](/configuration/yarnrc#field-logfilters):

```yaml
logFilters:
  - pattern: "*copied instead of hard-linked*"
    level: discard
```

Use `level: info` instead to keep the message without it being reported as a warning.
