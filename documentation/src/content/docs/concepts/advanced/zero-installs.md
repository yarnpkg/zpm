---
category: concepts
slug: concepts/zero-installs
title: Zero Installs
description: A way to make a project install-free.
sidebar:
  order: 11
---

Working on a high-velocity project comes with its own set of challenges. One of those is the constant need to reinstall dependencies whenever you change branches. Yarn came up with an opt-in pattern addressing this problem, which we call Zero Installs.

:::caution
Zero-Installs come with their drawbacks which are documented below. Modern versions of Yarn implement by default an alternate approach called Lazy Installs that we believe scale better on very large projects. Nonetheless Zero Installs can be a useful option for smaller internal projects.
:::

## How does it work?

The idea of Zero Installs is extremely simple: what if all install artifacts were checked-in to version control? This way, when you clone a repository, you already have all the necessary files to run the project without needing to install anything. Changing branches would also be seamless, as your VCS will automatically update the install artifacts as it performs a checkout.

While simple in appearance, I'm sure you'll quickly raise questions about its feasibility: checking in all install artifacts in a typical Node.js project means checking-in your `node_modules` directory. This can be a significant overhead, especially for large projects with many dependencies. This issue is exacerbated by the way `node_modules` hoisting works, which can lead to files being arbitrarily moved around as you add & remove dependencies, creating massive diffs in your PRs.

That's true, and that's why we don't recommend checking-in your `node_modules` directory. That's where [Yarn Plug'n'Play](https://yarnpkg.com/concepts/pnp) comes to the rescue! Under this mode, Yarn doesn't generate a `node_modules` directory. Instead, a single file is generated called the `.pnp.cjs` file (plus another called `.pnp.loader.mjs` for ESM support).

These files are [Node.js loaders](https://nodejs.org/api/module.html#customization-hooks) that contains a mapping of all dependencies to their respective locations on disk, allowing Yarn to resolve dependencies without needing to generate a `node_modules` directory. Their content is deterministic, so they can safely be checked-in to version control.

Those loaders are one key to Zero Installs, but not the only one. The `.pnp.cjs` file will contain by default references to dependencies from your global filesystem cache. This cache is unique to your machine, so if someone else uses it they will probably be missing some packages. But Yarn has a way to address that, thanks to the `enableLocalCache` option.

With this setting set, Yarn will keep your project's cache into your project, in the `.yarn/cache` directory. Thanks to that, any package you add to your project will be stored as a separate unique zip file in that directory. And while you might think keeping binary files into your repository is an unfathomable idea, it turns out Git providers are perfectly fine with this pattern.

It also solves the issues we discussed with checking-in `node_modules` folders:

- The Yarn Plug'n'Play dependency tree is guaranteed to be perfectly flat, so no package will ever be updated just because you add or remove unrelated dependencies.

- Zip archives can be stored uncompressed, allowing Git to compute deltas between versions.

- Each third-party package is tracked as a single file, making it easy for Git to track changes.

## Limitations

What I write here is based on my experience with an internal repository I worked on. It used the strategy described in this paper for more than five years until we switched to Lazy Installs.

Keep in mind that this repository had an impressive scale (we're talking thousands of workspaces), and was very active (30+ `yarn.lock` updates per day). That Zero Install worked at all for so long demonstrates that the pattern is viable, especially when starting right away with the mitigations we discovered along the way.

So with that context in mind, here are the challenges we faced:

- Some Yarn updates required regenerating all of the cache files, which Git didn't like. This is less of an issue on modern releases, as we control much better the byte representation of the cache files, and can avoid unnecessary updates.

- We discovered that storing files uncompressed in Git was better than storing them compressed. As an example this [uncompressed repo](https://github.com/yarnpkg/example-repo-zip0) weights [1.25GiB](https://api.github.com/repos/yarnpkg/example-repo-zip0), whereas [this identical compressed one](https://github.com/yarnpkg/example-repo-zipn) weights [2.1GiB](https://api.github.com/repos/yarnpkg/example-repo-zipn).

- We also found out that the zip cache is only part of the story, and not actually the heaviest contributor to the repository size. Surprisingly, the actual heaviest contributor was the `.pnp.cjs` file. This was due in part to the very large amount of workspaces and inter-dependencies (leading to the `.pnp.cjs` file being almost 25MiB), but also to Git's delta algorithm being inefficient at dealing with that update pattern.

- Generally speaking, the issues were mostly that once a problem was spotted, it was difficult to fully address it, as it would have required rewriting the full commit history.
