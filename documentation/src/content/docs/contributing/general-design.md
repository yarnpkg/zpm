---
category: contributing
slug: contributing/design
title: General design
description: Overview of the design decisions made in Yarn.
sidebar:
  order: 3
---

The ZPM codebase shares a similar high-level design with the Berry codebase. Installs work as follows:

1. We start with a queue containing a set of root descriptors. Typically that will be the descriptors for the workspaces in the project.

2. For each descriptor, we pass it down to the `resolve_descriptor` function, which will select the proper resolver function for the given range. This resolver function will then return us a resolution object.

3. Each resolution we receive triggers two events:

    - First, we enqueue new descriptors into our queue for each dependency listed in the resolution.

    - Second, we forward the resolution's locator to the [`fetch_locator`](https://github.com/yarnpkg/zpm/blob/main/packages/zpm/src/fetchers/mod.rs) function. It will select the proper fetch function for the given reference, which will then pull the package data (either as a cache reference, or local path reference).

4. Once we have finished resolving all descriptors and fetching all locators, we enter the link phase by calling [`link_project`](https://github.com/yarnpkg/zpm/blob/main/packages/zpm/src/linker/mod.rs). What happens there changes depending on the configured [linker](/concepts/node-linkers), but in the end they yield two new pieces of information:

    - A list of locations on disk for each package in the dependency graph. For the Yarn PnP and pnpm linkers we'll have exactly one location per locator, whereas the `node_modules` linker may return multiple locations per locator to satisfy the hoisting.

    - A list of "build requests", ie instructions on how to build the packages that have been laid out on disk. These requests can depend on other requests.

5. The core will then take all the build requests and process them, parallelizing the builds as much as possible while still respecting the dependencies between them.
