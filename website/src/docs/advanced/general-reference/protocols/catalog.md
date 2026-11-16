---
category: protocols
slug: protocol/catalog
title: Catalog Protocol
description: How catalog dependencies work in Yarn.
---

The `catalog:` protocol lets dependencies reference version ranges declared in `.yarnrc.yml` instead of repeating the same ranges across many manifests.

```yaml
catalog:
  react: ^19.0.0

catalogs:
  tooling:
    typescript: ^5.0.0
```

```json
{
  "dependencies": {
    "react": "catalog:",
    "typescript": "catalog:tooling"
  }
}
```

`catalog:` without a name reads from the default `catalog` setting. `catalog:name` reads from the matching entry in `catalogs`.

Catalog entries are resolved during install to the range configured for the dependency ident. If the named catalog or dependency entry doesn't exist, Yarn reports an error rather than falling back to another range.

## Peer dependencies

Workspaces can also use `catalog:` and `catalog:name` in their `peerDependencies`. The peer range is then the range stored in the catalog for that dependency; it must be a semver range (such as `^19.0.0` or `npm:^19.0.0`), and Yarn reports an error if the catalog or the entry doesn't exist.

```json
{
  "peerDependencies": {
    "react": "catalog:"
  }
}
```

Packages are expected to never contain `catalog:` ranges once published: `yarn pack` and `yarn npm publish` replace them with the range from the catalog, in `dependencies`, `devDependencies`, `optionalDependencies` and `peerDependencies` alike. A `catalog:` peer range found in a package that isn't part of the project is ignored and treated as `*`.
