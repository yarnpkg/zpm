---
category: contributing
slug: contributing/data-structures
title: Data structures
description: Overview of the data structures used in Yarn.
sidebar:
  order: 2
---

Various data structures should be used when working with the Yarn codebase. Most of these will be familiar to anyone who has worked with the Berry codebase, but some are new.

## Core primitives

- [**Ranges**](https://github.com/yarnpkg/zpm/blob/main/packages/zpm-primitives/src/range.rs) are enumerations used to represent a set of potential packages. Yarn supports a variety of ranges, the most common being semver ranges but also git ranges, http ranges, file ranges, etc.

- [**References**](https://github.com/yarnpkg/zpm/blob/main/packages/zpm-primitives/src/reference.rs) are enumerations as well. They are very much like ranges, but they only ever represent a single package. For this reason some ranges don't have a direct mapping to references (instead we rely on resolvers to convert them), but references can always be converted back to ranges.

- [**Idents**](https://github.com/yarnpkg/zpm/blob/main/packages/zpm-primitives/src/ident.rs) are structs that represent package names. They are a combination of a package scope and a package name.

- [**Descriptors**](https://github.com/yarnpkg/zpm/blob/main/packages/zpm-primitives/src/descriptor.rs) are structs that represent the combination of an ident and a range. The `dependencies` field of a `package.json` file is a collection of descriptors.

- [**Locators**](https://github.com/yarnpkg/zpm/blob/main/packages/zpm-primitives/src/locator.rs) are similar to descriptors in that they represent the combination of an ident and a reference. They are used to uniquely identify a package within a project.

## Miscellaneous primitives

- [**LooseDescriptor**](https://github.com/yarnpkg/zpm/blob/main/packages/zpm/src/descriptor_loose.rs) represent a potentially incomplete descriptor that Yarn first needs to resolve. This is for example the type that `yarn add <pkg>` accepts: `<pkg>` can be a package name, a package name and its version, a git URL, etc.

- [**Resolutions**](https://github.com/yarnpkg/zpm/blob/main/packages/zpm/src/resolvers/mod.rs) contain both a locator and additional metadata about the package (such as its version, its dependencies, etc). The lockfile is a serialized list of resolution entries.

  - Resolutions can only store metadata that we could retrieve from the npm registry metadata endpoints, as they are pulled before the package is actually fetched.
