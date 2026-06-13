---
category: contributing
slug: contributing/building
title: Building & testing
description: How to build Yarn from sources and run its tests.
sidebar:
  order: 1
---

## Installing dependencies

| Software | How to install |
| --- | --- |
| Yarn | [https://yarnpkg.com/getting-started/install](/getting-started/install) |
| Rust / Rustup | https://rust-lang.org/tools/install |

## Building Yarn

Clone the ZPM repository and cd into it:

```bash
git clone https://github.com/yarnpkg/zpm.git
cd zpm
```

You should now be able to build the project:

```bash
cargo build --release -p zpm-switch -p zpm
```

We tend to build Yarn in release mode, even in development, because Rust is known to be significantly slower in debug mode. Regardless of whether you want to use the release or debug version, create a symbolic link named `local` pointing to the binary you just created:

```bash
ln -s target/release local
```

Also configure your system's Yarn Switch to use this local version when working on the project:

```bash
yarn switch link target/release/yarn-bin
```

## Testing Yarn

The CLI integration tests run against the binary you just built. From the repository root, run:

```bash
cargo build -r
yarn test:integration <jest options>
```

For example, to run only the install command tests:

```bash
yarn test:integration commands/install.test.ts
```

Ecosystem end-to-end tests are shell scenarios under `tests/e2e`. Run them by name, without the `.sh` suffix:

```bash
yarn test:e2e <e2e test name>
```
