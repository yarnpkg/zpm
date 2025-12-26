---
category: concepts
slug: concepts/nvm
title: Node.js versioning
description: Pin the version of Node.js used in your application
sidebar:
  order: 3
---

One of the subtle causes of "works on my machine" bugs comes from differences in Node.js versions between developers. While tools like nvm, fnm, or Volta help manage local Node.js installations, they require each team member to manually configure their environment. Yarn takes a different approach by letting you declare Node.js as a project dependency, ensuring everyone uses exactly the same version without any extra setup.

## The `@builtin/node` dependency

Yarn provides a special `@builtin/node` package that you can add to your dependencies just like any other package. When installed, Yarn will download the appropriate Node.js binary for your platform directly from [nodejs.org](https://nodejs.org/) and make it available to your project.

```json
{
  "name": "my-app",
  "dependencies": {
    "@builtin/node": "^22.0.0"
  }
}
```

The range here works similarly to semver ranges - Yarn will resolve it to the highest available Node.js version that satisfies your constraint. Once resolved, this version gets locked in your `yarn.lock` file, guaranteeing that every developer and CI environment uses the exact same Node.js release.

## Why manage Node.js through Yarn?

There are several advantages to managing Node.js as a dependency:

- **Zero configuration** - CI and team members don't need to install nvm or any other version manager. Just `yarn install` and they're ready to go.

- **Version locking** - The exact Node.js version is recorded and cached along with all other dependencies in your project.

- **Per-project versions** - Different projects can use different Node.js versions without any manual switching. Yarn handles it automatically.

- **Per-workspace versions** - You can easily override the Node.js version to use for a single workspace or a set of workspaces through [profiles](/concepts/profiles).

## Using the managed Node.js

Once installed, the managed Node.js binary is available through the `node` binary that Yarn injects into your environment. You can use it in several ways:

### Through Yarn scripts

Package scripts automatically use the project's Node.js version:

```json
{
  "scripts": {
    "start": "node server.js"
  }
}
```

### Through `yarn node` / `yarn exec`

Both commands will run Node.js with the correct environment setup:

```bash
yarn node --version
yarn node script.js
yarn exec node --version
```

## Monorepo support

In a monorepo, you typically want all workspaces to use the same Node.js version. Rather than adding `@builtin/node` to each workspace's `package.json`, you can use [workspace profiles](/concepts/profiles) to declare it once:

```yaml
workspaceProfiles:
  default:
    devDependencies:
      "@builtin/node": "builtin:^22.0.0"
```

Since the `default` profile is automatically applied to all workspaces, every package in your monorepo will use the same Node.js version without any additional configuration. This keeps your Node.js version centralized and easy to update.

## Platform support

The `@builtin/node` package automatically downloads the correct binary for your operating system and architecture. Currently supported platforms include:

- Linux (x64, arm64)
- macOS (x64, arm64)

When working in a team with mixed platforms, Yarn will store metadata about all required platform variants in the lockfile, but each developer will only downloads the binary they need for their platform (configurable through `supportedArchitectures`).

This ensures the lockfile remains consistent across the entire team while keeping installs as lightweight as possible.
