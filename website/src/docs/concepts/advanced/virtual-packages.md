---
category: concepts
slug: concepts/virtual-packages
title: Virtual packages
description: An explanation of virtual packages, why they are necessary, and how to keep them in check.
sidebar:
  order: 7
---

:::caution
What follows is an advanced topic that isn't strictly necessary to understand how peer dependencies work. It however provides a deeper understanding of how Yarn manages the dependency graph.
:::

## Prior context

First, let's clarify a point of detail about how Yarn works. Before peer dependencies are processed, Yarn generates a graph in which each node represents a package, and each dependency represents an edge. Both as an optimization and to make the lockfile more readable, Yarn ensures that identical dependencies always point to the same node in the graph. So if you have a package listing a dependency `"foo": "^1.0.0"` and another package with the *exact* same dependency, Yarn will ensure that both `foo` dependencies will point to the same node (same version).

It works perfectly for regular dependencies, but peer dependencies shatter that model. The problem we face is that a single package (let's say `my-react-component`) listing a peer dependency (on `react`) is no longer unique. Depending on which package is its ancestor (let's say either `web`, which provides `react@19`, or `mobile`, which provides `react@18`), `my-react-component` may end up connected to different versions of `react`. We can't just connect `web` and `mobile` to the same `react` node, because we'd have no way to decide whether `my-react-component` should be connected to `react@19` or `react@18`.

## Virtual packages

To solve this problem Yarn introduces the concept of *virtual packages*, which are unique copies of the `my-react-component` node created by the graph resolver for each time `my-react-component` was found while traversing the dependency graph. Each copy will have access to a different set of peer dependencies.

Taking the example above:

- The `web` node will be connected to `my-react-component#1`, itself connected to `react@19`
- The `mobile` node will be connected to `my-react-component#2`, itself connected to `react@18`

Those virtual packages have different representations on disk depending on your [linker](/concepts/linkers):

- The node-modules and pnpm linkers will duplicate those packages on disk.

- The Yarn Plug'n'Play linker will keep those packages virtual; each of them will be assigned unique "virtual paths" (ie `/my/project/.yarn/__virtual__/...`), but they will all turn into the same path before Node.js performs the actuall syscalls. This is similar in idea to symlinks, but without actually being symlinks to prevent Node.js resolving them when passing file names to `realpath` before `import` calls.

:::note
You may wonder *"why is this so complicated in Yarn? Why doesn't it work just like npm and pnpm, which don't need all that complexity?"* - that's because their only partially support peer deps!

Due to the reliance on the filesystem, both node-modules and pnpm installs are unable to enforce the peer dependency contract with workspaces. This limitation is referred to as the nm / peer deps issue, which is documented [here](/appendix/nm-peer-deps).
:::

## Package duplication

We saw that Yarn will create a virtual package for each peer dependency set so that each package gets exactly what it should per the dependency graph. This is all fine when everything is working as expected, but it comes with challenges.

For one, it goes both ways: while you can be sure that all virtual packages will get exactly what you provide, it also means you have to be careful about the peer dependencies you provide. If you're not careful you may cause a package to be duplicated one or more times in separate virtual package instances. This can lead to worse runtime performances, broken `instanceof` checks, and broken features relying on shared data structures (such as React contexts).

Another issue are dependency cycles. Yarn will optimize the dependency graph to avoid keeping multiple copies of the same virtual package with the same peer dependency sets, but that strategy has limits. We haven't found a satisfying way to deduplicate that depend on each other.
