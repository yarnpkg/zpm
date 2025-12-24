---
category: concepts
slug: concepts/profiles
title: Workspace profiles
description: Reuse configuration between your workspaces
sidebar:
  order: 4
---

When working with monorepos, you often find yourself repeating the same dev dependencies across multiple workspaces. Maybe all your TypeScript packages need `@types/node`, or all your React packages need `@testing-library/react`. Instead of duplicating these dependencies in every `package.json`, workspace profiles let you define reusable sets of dev dependencies that can be shared across workspaces.

## Defining profiles

Profiles are defined in your `.yarnrc.yml` file under the `workspaceProfiles` key. Each profile can contain dev dependencies and can extend other profiles:

```yaml
workspaceProfiles:
  typescript:
    devDependencies:
      "@types/node": "^20.0.0"
      "typescript": "^5.0.0"

  react:
    devDependencies:
      "@testing-library/react": "^14.0.0"
      "react": "^18.0.0"

  fullstack:
    extends:
      - typescript
      - react
    devDependencies:
      "eslint": "^8.0.0"
```

## Using profiles

To apply a profile to a workspace, add an `extends` field to its `package.json`:

```json
{
  "name": "my-package",
  "extends": ["typescript"]
}
```

You can extend multiple profiles, and they will all be merged together. The `default` profile is always automatically included, even if you don't specify it explicitly.

## Profile inheritance

Profiles can extend other profiles, allowing you to build up complex configurations from simpler building blocks. When a profile extends another, all of its dev dependencies are inherited. If multiple profiles define the same dependency, the workspace's own `devDependencies` take precedence, followed by profiles applied later in the resolution order.

## Limitations

- Profiles only apply to dev dependencies. Regular dependencies must still be declared in each workspace's `package.json`.

- If a workspace already declares a dev dependency in its `package.json`, profiles won't override it. This ensures that workspace-specific requirements always take precedence over shared profiles.
