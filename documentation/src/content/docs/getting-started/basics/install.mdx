---
category: getting-started
slug: getting-started
title: Installation
description: Yarn's in-depth installation guide.
sidebar_position: 2
---

Yarn is intended to be used with [Yarn Switch](/concepts/switch), a tool that lets you use different Yarn versions depending on the project you're in.

```bash
curl -sS https://repo.yarnpkg.com/install | bash
```

:::caution
You will likely have to restart your terminal sessions, **IDEs included**, for the changes to take effect.
:::

Once Yarn Switch is ready, if you wish to start a new project, running `yarn init` is all that's needed. If you instead prefer to migrate an existing project, run the following command:

```
yarn switch zpm set version zpm
```

It intructs Yarn Switch to download the latest version of Yarn on its `zpm` release branch and run the `yarn set version zpm` command with it. This command will in turn update `packageManager` with the appropriate version.

## Updating Yarn

Any time you'll want to update Yarn to the latest version, just run:

```bash
yarn set version latest
```

Yarn will then configure your project to use the most recent stable binary.

## Alternative installs

### Corepack

While Corepack doesn't support installing different binaries depending on the platform, we provide a compatibility setting under the form of `YARNSW_COREPACK_COMPAT` which, when set, will instruct Corepack to install Yarn Switch and transparently forward Yarn CLI commands to it.

While not recommended for a local setup, this compatibility layer can unlock you if you work in environments where dependencies are installed with Corepack before you even have a chance to run your own code, such as Netlify runners.

### GitHub Actions

We provide a `yarnpkg/setup-action` repository that runs the standard installation flow on GitHub Actions:

```yaml
jobs:
  my-job:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Setup Yarn
        uses: yarnpkg/setup-action@main
```

### Self-managed binaries

Our [release tarballs](https://github.com/yarnpkg/zpm/releases) all provide two binaries:

- `yarn` is the [Yarn Switch](/concepts/switch) binary, which adds support for reading the `packageManager` field.

- `yarn-bin` is the "true" binary, containing all commands such as `yarn install`, `yarn add`, etc.

You can bypass Yarn Switch by extracting the `yarn-bin` binary somewhere into your `$PATH` and renaming it into `yarn`.
