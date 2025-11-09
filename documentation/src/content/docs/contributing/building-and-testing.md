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

One of the reasons why the migration from the Classic codebase to the Berry one was so painful was that we lost all our testing framework. All Classic tests were written using internal primitives, so they couldn't be reused after the redesign.

We learned from that mistake, and the Berry tests were written using the regular CLI as interface. This means it's easy to swap the binary from Berry to ZPM and run the full Yarn testsuite!

Start by cloning the Berry repository:

```bash
git clone https://github.com/yarnpkg/berry.git ~/berry
```

Export a `BERRY_DIR` environment variable pointing to the Berry repository:

```bash
export BERRY_DIR=~/berry
```

Then run the `yarn berry` command from the ZPM repository:

```bash
yarn berry test:integration commands/add.test
```
