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
