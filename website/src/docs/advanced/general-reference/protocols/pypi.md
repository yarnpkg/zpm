---
category: protocols
slug: protocol/pypi
title: PyPI Protocol
description: How PyPI dependencies work in Yarn.
---

The `pypi:` protocol fetches Python packages from a PyPI-compatible registry and stores the selected wheel in the Yarn cache. It can be used when a project needs to install Python packages alongside its JavaScript dependencies.

```json
{
  "dependencies": {
    "pypi-no-deps": "pypi:1.0.0",
    "pypi-one-dep": "pypi:>=1.0.0,<2.0.0"
  }
}
```

The range part uses Python package version specifiers. Yarn resolves it against the package metadata, selects the best compatible wheel, and records the exact artifact in the lockfile.

## Registry

By default Yarn queries `https://pypi.org`. You can override this endpoint with the `pypiRegistryServer` setting, or for specific packages with `packageRules`.

```yaml
pypiRegistryServer: https://pypi.example.org
```

## Package names

The dependency name in your manifest is the package name exposed to the JavaScript project. When needed, you can point it at a different PyPI package name by placing that name after the protocol prefix.

```json
{
  "dependencies": {
    "local-name": "pypi:remote-name@1.0.0"
  }
}
```
