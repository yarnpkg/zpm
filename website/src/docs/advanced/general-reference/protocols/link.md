---
category: protocols
slug: protocol/link
title: Link Protocol
description: How link dependencies work in Yarn.
---

The `link:` protocol lets you connect your project to an external directory.

```
yarn add imgs@link:./static/imgs
```

## Links vs portals

Since the target referenced by `link:` may not exist until postinstall scripts have run, and since Yarn guarantees a predictable behavior regardless of the execution order, we need to disambiguate _packages_ (which contain `package.json` files, and may list dependencies of their own) from _arbitrary folders_ (which may not).

In practice, this means that `link:` dependencies are treated as arbitrary folders. They may point to folders that contain a `package.json`, but Yarn won't use that manifest to resolve dependencies or run package scripts. If you need to symlink to a _package_ with its own dependency metadata, use [portals](/protocol/portal) instead.
