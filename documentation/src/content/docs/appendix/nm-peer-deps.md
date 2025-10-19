---
category: appendixes
slug: appendix/nm-peer-deps
title: The nm / peer deps issue
description: An overview of incompatibilities between workspaces and peer dependencies when using the node_modules or pnpm installation strategies.
---

Consider the workspaces below. The resulting hoisting will be something like this:

- `node_modules/react` will have version 18, hoisted from `packages/web`
- `packages/mobile/node_modules/react` can't be hoisted and will have version 19
- `packages/mobile/node_modules/component-lib` will be a symlink to `packages/component-lib`

But this tree is invalid: because the `node_modules` resolution algorithm [always resolves symlinks](https://github.com/nodejs/node/issues/3402) before importing modules, the `component-lib` workspace will always require `react` in its version 18, even when accessed through `mobile`. This is despite `mobile` correctly providing the `react` dependency in version 19 through the peer dependency.

There is unfortunately no `node_modules` layout that can address that correctly.

:::note
Pnpm provides [`injectWorkspacePackages`](https://pnpm.io/settings#injectworkspacepackages) to workaround this issue by making hardlinked copies of `component-lib` into both `mobile` and `web`, but this requires more filesystem operations, and the copies need to be periodically synced to add new files and remove old ones.
:::

### packages/component-lib

```json
{
  "name": "component-lib",
  "peerDependencies": {
    "react": "*"
  }
}
```

### packages/web

```json
{
  "dependencies": {
    "component-lib": "workspace:*",
    "react": "^18"
  }
}
```

### packages/mobile

```json
{
  "dependencies": {
    "component-lib": "workspace:*",
    "react": "^19"
  }
}
```
