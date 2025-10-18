---
category: concepts
slug: concepts/constraints
title: Constraints
description: Yarn constraints, a way to enforce common rules across a project.
---

Constraints are a powerful feature in Yarn that allow you to define and enforce rules across your project. They can be used to validate the structure of your `package.json` files, raise errors when they are not met, and even declare fixes to automatically apply.

Unlike Eslint-based linting, constraints have access to the project's entire dependency tree, allowing them to enforce rules that would be difficult to implement with static analysis alone - think circular dependencies or version consistency checks.

## Definining constraints

Constraints are created by adding a `yarn.config.cjs` file at the root of your project (repository). This file should export an object with a `constraints` method. This method will be called by the constraints engine, and must define the rules to enforce on the project, using the provided API. For example:

### Enforcing dependency versions

```ts
module.exports = {
  async constraints({ Yarn }) {
    for (const dep of Yarn.dependencies({ ident: "react" })) {
      dep.update(`18.0.0`);
    }
  },
};
```

### Enforcing package.json fields

```ts
module.exports = {
  async constraints({ Yarn }) {
    for (const workspace of Yarn.workspaces()) {
      workspace.set("engines.node", `20.0.0`);
    }
  },
};
```

## Declarative model

Constraints are defined using a declarative model: you declare what the expected state should be, and Yarn checks whether it matches the reality or not. If it doesn't, Yarn will either throw an error (when calling `yarn constraints` without arguments) or attempt to fix the issue (when calling `yarn constraints --fix`).

Because of this declarative model, you shouldn't check the actual values yourself. For instance, the check here is extraneous and should be removed:

```ts
module.exports = {
  async constraints({ Yarn }) {
    for (const dep of Yarn.dependencies({ ident: "ts-node" })) {
      // No need to check for the actual value! Just always call `update`.
      if (dep.range !== `18.0.0`) {
        dep.update(`18.0.0`);
      }
    }
  },
};
```

## TypeScript support

Yarn provides a type package to make it easier to write constraints. To use them, first add the package to your top-level dependencies:

```
yarn add @yarnpkg/types
```

Then import the `defineConfig` function in your `yarn.config.cjs` file:

```ts
/** @type {import('@yarnpkg/types')} */
const { defineConfig } = require("@yarnpkg/types");

module.exports = defineConfig({
  async constraints({ Yarn }) {
    // `Yarn` is now well-typed ✨
  },
});
```

You can also retrieve the types manually, which can be useful if you extract some rules into helper functions:

```ts
/** @param {import('@yarnpkg/types').Yarn.Constraints.Workspace} dependency */
function expectMyCustomRule(dependency) {
  // ...
}
```

You can alias the types to make them a little easier to use:

```ts
/**
 * @typedef {import('@yarnpkg/types').Yarn.Constraints.Workspace} Workspace
 * @typedef {import('@yarnpkg/types').Yarn.Constraints.Dependency} Dependency
 */

/** @param {Workspace} dependency */
function expectMyCustomRule(dependency) {
  // ...
}
```
