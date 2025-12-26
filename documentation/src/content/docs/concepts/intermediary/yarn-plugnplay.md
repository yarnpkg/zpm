---
category: concepts
slug: concepts/pnp
title: Yarn Plug'n'Play
description: An overview of Yarn Plug'n'Play, a powerful and innovative installation strategy for Node.js.
sidebar:
  order: 5
---

Yarn Plug'n'Play, also known as Yarn PnP, is the default installation strategy in modern releases of Yarn. While it can be swapped out for more traditional strategies such as `node_modules` or pnpm-style symlink-based installs, we recommend it when creating new projects.

## First some context

The only builtin resolution strategy in Node.js at this point in time is the `node_modules` one. When performing a resolution, Node.js will look for the package in the current directory's `node_modules` folder, then in the parent directory's `node_modules` folder, and so on until it reaches the root directory. The first directory it finds that contains the file will be used.

This approach is simple, but comes with some limitations. For one, a naive package layout where each package simply contains its own dependencies would lead to a massive `node_modules` footprint, and would often [break path length limits](https://github.com/npm/npm/issues/3697).

The main optimization is called hoisting. An hoisted `node_modules` tree doesn't just contain its own dependencies - it also contains the dependencies of its dependencies, and so on. This neat trick removes a lot of package duplication, but can't fully address the problem - multiple versions of the same package can't coexist in the same directory, so package managers have to duplicate them based on heuristics.

Another major issue is that hoisting allows each package to import not only its own dependencies, but also any other package that happens to have been hoisted in its `node_modules` directory. This issue, where a package accidentally imports a dependency that isn't listed in its `package.json`, is often referred to as "ghost dependencies".

Ghost dependencies lead to unexpected behaviors and bugs as the addition or removal of even a single unrelated package can impact our hoisting heuristics and drastically reorganize the `node_modules` layout.

To address these issues, other package managers such as pnpm came up with improvements. Thanks to a smart use of symlinks those package managers can avoid some ghost dependencies by creating entirely separate `node_modules` branches for each package.

While being a significant improvement over the naive `node_modules` approach, this strategy doesn't solve everything. For one it still involves a large amount of filesystem operations to generate symlinks and copy files. It also isn't able to represent all [dependency trees](/appendix/nm-peer-deps).

## How does Plug'n'Play work?

Yarn Plug'n'Play works by creating a [Node.js loader](https://nodejs.org/api/module.html#customization-hooks) instead of `node_modules` folder. This loader contains a map of all packages and their dependencies, along with their locations on disk. This map is used to resolve imports at runtime, avoiding the need to query the file system.

Because our map contains the whole dependency tree, we can easily check that a package only accesses dependencies it declares in its `package.json`. This ensures that no ghost dependencies are introduced, and that the package's behavior is predictable and consistent.

## Ecosystem compatibility

While our loader integrates perfectly with the standard Node.js resolution APIs such as [`require.resolve`](https://nodejs.org/api/modules.html#requireresolverequest-options), [`createRequire`](https://nodejs.org/api/module.html#modulecreaterequirefilename) or [`import.meta.resolve`](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Operators/import.meta/resolve), we don't have `node_modules` folders. As a result, third-party packages or tools that make assumptions about their presence may have issues. Two examples:

- Packages that accidentally read into the `node_modules` folder, for example to load packages starting with a given prefix as plugins. Those packages usually degrade gracefully, as they often also offer their users to be explicit about the plugins they want to use.

- Tools that implement their own dependency resolution logic rather than using the standard Node.js APIs. This is often the case with bundlers and linters as they need to support various features that Node.js wouldn't otherwise support (`browser` or `types` fields, etc).

In that last case we worked with the relevant teams to implement native support Yarn Plug'n'Play in their pipeline. This work was made easier thanks to the [Plug'n'Play specification](https://yarnpkg.com/features/pnp) and the [`pnp-rs` crate](https://github.com/yarnpkg/pnp-rs), which explain how to implement Plug'n'Play support outside of Node.js environments.

Today, Yarn Plug'n'Play is supported natively by Vite, Webpack, Esbuild, Rspack, Eslint, and many more.

## Frequently asked questions

### How can I fix dependencies?

Unlike the `node-modules` and `pnpm` linkers, accessing ghost dependencies under the Plug'n'Play strategy will throw an exception letting you know of the issue, leaving it up to you to decide how to proceed.

The easiest way to fix such dependencies is by using the `packageExtensions` setting; it allows you to inject new dependencies into any package from your dependency tree. For example, should you face an error such as `@babel/core tried to access @babel/types, but it isn't declared in its dependencies`, you could easily fix it by adding the following to your `.yarnrc.yml` file:

```yaml
packageExtensions:
  "@babel/core@*":
    dependencies:
      "@babel/types": "*"
```

:::note
It may sometimes make sense to extend the `peerDependencies` field rather the `dependencies` field, this is to be addressed case-by-case.
:::

:::tip
To avoid you having to maintain a large set of `packageExtensions` entries, the Yarn team maintains a list of [known ghost dependencies in the ecosystem](https://github.com/yarnpkg/berry/blob/master/packages/yarnpkg-extensions/sources/index.ts) that Yarn automatically applies. This list is shared between Yarn and pnpm, and we're more than happy to merge contributions.
:::
