---
category: getting-started
slug: getting-started/migrating-from-uv
title: Migrating from uv
description: Install your uv projects with Yarn, in the same monorepo as your JavaScript projects.
---

Yarn can install Python projects: each project becomes a workspace whose dependencies use the [`pypi:` protocol](/protocol/pypi), resolved by an island using the `venv` linker. Running `yarn install` creates a regular `.venv` in each project, usable by any Python tool.

## Importing projects

Run `yarn import uv` from the root of your Yarn project, passing the folders containing a `pyproject.toml`:

```sh
yarn import uv services/api --recursive --dry-run   # preview
yarn import uv services/api --recursive
yarn install
```

For each project, the command writes a `package.json` next to its `pyproject.toml`, adds it to the root `workspaces`, and registers an island in `.yarnrc.yml`. With `--recursive`, the local projects it depends on (path sources, uv workspace members) are imported as well.

The `pyproject.toml` and `uv.lock` files are left untouched; you can keep using uv while migrating.

| uv | Yarn |
| --- | --- |
| `project.dependencies` | `dependencies` (`pypi:` ranges) |
| `dependency-groups.dev`, `tool.uv.dev-dependencies` | `devDependencies` (other groups with `--group <name>`) |
| `pkg[extra]` | `pypi:<range>#extras=extra` |
| `pkg; sys_platform == 'darwin'` | `pypi:<range>#marker=...` |
| `tool.uv.sources` with `path` or `workspace = true` | `workspace:^`, installed in editable mode |
| `tool.uv.sources` with `index` | a `packageRules` entry routing the package to the index |
| `tool.uv.override-dependencies` | the workspace's `resolutions` (applied to its island) |
| `tool.uv.constraint-dependencies` | the island's `pypiConstraints` |
| `requires-python` | the island's `pythonVersion` |
| `uv.lock` | the island's `pypiSeedLockfile`: the first install prefers the versions uv had locked |
| `exclude-newer = "2 days"` | `pypiMinimalAgeGate: 2d` (global) |
| `[[tool.uv.index]]` with `default = true` | `pypiRegistryServer` |
| `UV_INDEX_<NAME>_PASSWORD` | `pypiAuthToken` / `pypiAuthIdent` with `${ENV_VAR}` interpolation |

Optional dependencies aren't copied into the manifest: Yarn reads them from `pyproject.toml` when a dependent requests them (`mylib[server]` adds the requirements of that extra to the dependent).

## Configuration

A typical configuration, with a public mirror as default index and a private index for internal packages:

```yaml
pypiRegistryServer: https://socket-registry.example.com/pypi/simple/
pypiMinimalAgeGate: 2d
pythonVersion: "3.12"

# Lock wheels for the developers' and CI's platforms
supportedArchitectures:
  - {os: [darwin], cpu: [arm64]}
  - {os: [linux], cpu: [x64, arm64], libc: [glibc]}

packageRules:
  - ecosystemFilter: pypi
    packageFilter: "mycompany-*"
    pypiRegistryServer: https://pypi.fury.io/mycompany/
    pypiAuthToken: ${GEMFURY_TOKEN:-}
    pypiMinimalAgeGate: 0s

unstableIslands:
  api:
    workspaces: [api]
    linker: venv
    pypiSeedLockfile: services/api/uv.lock
```

## One island per project, or shared islands

`yarn import uv` creates one island per project, which reproduces uv's behavior: each project resolves its dependencies independently, and can pick versions other projects don't. Workspaces can also share an island (list several workspaces in its `workspaces` field): they then get a single version of each package, which is what a uv workspace does. Shared islands make upgrades consistent across projects, at the cost of having to agree on versions.

## Day-to-day usage

```sh
yarn install                 # creates/updates every .venv
yarn python -m pytest        # runs the workspace's venv Python
.venv/bin/pytest             # the venv also works without Yarn
yarn workspace api run test  # scripts run with the venv activated
```

IDEs only need to be pointed at `<project>/.venv/bin/python`.

## Differences with uv

- Markers are evaluated for the platforms listed in `supportedArchitectures` and the island's Python version, rather than solved for every possible environment: a lockfile generated for macOS and Linux won't contain Windows wheels unless Windows is listed.
- An island targets a single Python version (`pythonVersion`); uv's lockfiles can fork on `python_version` markers.
- Git and direct URL sources aren't supported yet; they're reported by the import command.
- Source distributions are built with pip's build isolation, inside a venv created from the island's interpreter.
