---
category: concepts
slug: concepts/peer-dependencies
title: Peer dependencies
description: A primer on peer dependencies, when to use them, and how to manage them.
sidebar:
  order: 3
---

Peer dependencies are often a feared tool due to subtle differences in how different package managers treat them. But they're also a powerful tool that addresses a very common problem: singleton packages. How to use them effectively is what we'll cover in this article.

## The dependency contract

Package managers turn the packages contained in your project into a dependency graph that's then turned into disk artifacts. The way the graph is constructed depends on the dependencies you declare. To make this process deterministic, package managers define a set of rules that govern how dependencies are connected to one another. We call these rules the dependency contract.

The most simple contract is the one enforced by dependencies listed in the `dependencies` field. Through this field the package informs the package manager that for the install to be valid, the package must be able to require the provided package and obtain in response a version that satisfies the semver range they requested (or the exact package they reference, in the case of [special protocols](/concepts/protocols)). That's it. Package managers are free to choose any way they want to satisfy it, as long as this cardinal rule is respected.

In particular, you'll note this requirement doesn't mention anything about other projects in the dependency graph. Since it's not part of the contract, package managers are under no obligation to ensure that the versions provided to the same dependencies from different packages are the same. This freedom isn't accidental - it's part of what allows package managers to compute efficient hoisting layouts.

## Peer dependencies

The problem however is that while regular dependencies work well enough when you don't care whether you use or not the same dependency as any other package, it sometimes happen that you do care. Take those cases:

- You write a development server and want to provide an integration for various optional packages. You don't want to install them all by default, but you want to be able to import them when present.

- You write a library (`my-react-component`) that uses primitives from another core library (`react`), and you need to ensure that you use exactly the same instance of that library as every other package that uses it to make sure global data structures (such as context) are shared.

- Your public interface relies on types provided from another library, and those types have high chance to be wildly different between different versions (`@types/node` or `@types/react`).

In those cases `dependencies` won't make the cut, because the package will just ensure that you get "a" version that matches what you asked for, but not necessarily the same version as other packages. That's where `peerDependencies` come in!

Peer dependencies may look similar to regular dependencies, but are quite different. For one, they only work with semver ranges (except for the `workspace:` and `catalog:` special protocols). That's because the package manager never "installs" them - it just adds an edge in the dependency graph that goes from your package to the relevant dependency node from your *parent package*.

Said another way, the peer dependency contract is written as such: "If the install is valid, a package with a peer dependency is guaranteed to obtain the exact same instance of this dependency when they import it as if the dependency was imported by the parent package in the dependency graph".

It's still a mouthful, but it's very simple at its core: instead of declaring the dependency yourself, you leave it up to the package that uses you to declare it. They are in control of the version you'll get (although you can still validate it matches your expectations by providing a semver range), and you can be sure that the version you get is the same as the version that your sibling packages will also get, provided they also declare the same peer dependency!
