---
category: concepts
slug: concepts/workspaces
title: Workspaces
description: A tour of what Yarn has to offer to monorepo projects.
---

Workspaces are a core part of Yarn's design, making it easy to manage multiple packages within the same repository - a pattern you may already be familiar with under the name of "monorepo".

In Yarn, each workspace is an isolated unit of that can reference other workspaces in its dependencies. As all workspaces have their own `package.json` file, they can choose whether to share scripts and dependencies with their siblings or not.

All projects use workspaces - even those that don't have multiple packages! The root workspace is always present, and it serves as the default workspace for the project.

## Why should I use a monorepo?

Monorepos offer several key benefits.

- Applying changes to multiple packages can be done in a single PR. This impacts refactorings, incentivizes modular code, encourages collaboration, and can simplify release processes that involve different environments like backend and frontend.

- Sharing information between workspaces is much easier than between multiple repositories. Ensuring that your frontend types are up-to-date with your backend types is usually trivial.

- Because you have a single source of truth, you can more easily understand what was the state of the world at any given time to trace changes and dependencies.

They also have a couple of drawbacks:

- It's very difficult to have different privacy settings between different parts of the repository. Unless your project is fully public, you'll likely need to extract your public dependencies into separate repositories.

- Good CI hygiene is essential to keep operations smooth. If some of the teams working on the monorepo aren't careful and let failing status checks reach production, they will be repercussed in every PR and may hinder your ability to merge changes.

From the author's experience, those drawbacks can be mitigated well enough that monorepos are a great choice for most projects, in both the open-source and corporate worlds.

## How to declare workspaces in Yarn?

Workspaces are declared in Yarn by adding a `workspaces` field to your `package.json` files, listing the workspaces' directories. This field accepts glob patterns, so you would usually put something like this:

```json
{
  "workspaces": [
    "packages/*"
  ]
}
```

## How to manage workspaces in Yarn?

Yarn puts various tools at your disposal to help you manage your project. These includes:

- The special [`catalog:`](/protocols/catalog) protocol lets you share dependency ranges between workspaces. Because this protocol is transparently replaced at publish-time, it's suitable for both public and internal packages.

- Another special protocol, [`workspace:`](/protocols/workspace), lets a workspace declare dependencies on other workspaces. Just like `catalog:` it's transparently replaced at publish-time.

- [Constraints](/concepts/constraints) let you ensure that your workspaces follow consistent rules. In a sense they can be seen as a linter / autofixer for monorepos.

- Our CLI accepts a path as optional first argument, provided it contains a `/`. As an example, running `yarn ./documentation vite` would run the `vite` binary in the `documentation` workspace.

Plus various workspace-related commands which you can find in our [CLI reference](http://localhost:4321/cli).
