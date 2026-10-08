---
category: protocols
slug: protocol/pypi
title: PyPI Protocol
description: How PyPI dependencies work in Yarn.
---

The `pypi:` protocol fetches Python packages from a PyPI-compatible index. Combined with a `venv` island, it lets Yarn install Python projects into regular virtual environments.

```json
{
  "dependencies": {
    "httpx": "pypi:>=0.27.0",
    "pydantic": "pypi:>=2.11,<3",
    "moto": "pypi:>=4.2#extras=s3,server",
    "urllib3": "pypi:>=2.7.0#marker=platform_python_implementation%20!%3D%20'PyPy'"
  }
}
```

The range uses [PEP 440](https://peps.python.org/pep-0440/) version specifiers (`*` matches anything). Pre-releases are only selected when the specifier mentions one, or when no final release matches. Parameters can be appended after a `#`:

| Parameter | Description |
| --- | --- |
| `extras` | Comma-separated list of extras to enable (`pkg[a,b]` in PEP 508). |
| `marker` | URL-encoded PEP 508 environment marker; the dependency is only installed where it evaluates to true. |

Package names are normalized following PEP 503 (`Foo_Bar` becomes `foo-bar`). To install a package under another name, put the PyPI name after the protocol: `"local-name": "pypi:remote-name@>=1.0"`.

## Indexes

Yarn talks to indexes through the [Simple Repository API](https://packaging.python.org/en/latest/specifications/simple-repository-api/): PEP 691 JSON is requested first, and PEP 503 HTML pages are supported as a fallback. `pypiRegistryServer` must point at the root of the Simple API (the URL you'd give to `pip --index-url`):

```yaml
pypiRegistryServer: https://pypi.org/simple/
```

Dependency metadata is read from the [PEP 658](https://peps.python.org/pep-0658/) `.metadata` files when the index advertises them, so resolving a package doesn't require downloading its wheels. Indexes that don't provide them are supported too, at the cost of downloading the artifact. Index listings and metadata are persisted in the global folder.

### Multiple indexes

Packages can be routed to another index through `packageRules`. The rule only applies to the packages it matches, so private packages are never looked up on the public index (and vice versa):

```yaml
pypiRegistryServer: https://pypi.org/simple/

packageRules:
  - ecosystemFilter: pypi
    packageFilter: "mycompany-*"
    pypiRegistryServer: https://pypi.example.com/simple/
    pypiAuthToken: ${PYPI_TOKEN:-}
```

### Authentication

| Setting | Header sent |
| --- | --- |
| `pypiAuthToken` | `Authorization: Bearer <token>` |
| `pypiAuthIdent` | `Authorization: Basic base64(<user:password>)` |

Both can be set globally, per package (`packageRules`), or per index (`sourceRules` with `ecosystemFilter: pypi` and `registryFilter: <index url>`). Values support environment variable interpolation (`${VAR}`, `${VAR:-default}`). Credentials are only sent to the host serving the index; artifacts hosted elsewhere (CDNs, S3 redirects) are downloaded without them.

### Age gate

`pypiMinimalAgeGate` ignores files uploaded more recently than the given duration (the equivalent of uv's `exclude-newer`). It relies on the upload times reported by PEP 691 indexes; files without an upload time (PEP 503 HTML indexes) are never filtered. It can be overridden per package or per index:

```yaml
pypiMinimalAgeGate: 2d

packageRules:
  - ecosystemFilter: pypi
    packageFilter: "mycompany-*"
    pypiMinimalAgeGate: 0s
```

Yanked files and files whose `requires-python` excludes the target Python version are never selected.

## Islands

Python dependencies are resolved by islands using the `venv` linker. An island resolves its workspaces together with a backtracking solver (PubGrub, using PEP 440 semantics), so each package has a single version per island - the same guarantee a uv project gives you.

```yaml
pythonVersion: "3.12"

unstableIslands:
  my-service:
    workspaces: [my-service]
    linker: venv
    pythonVersion: "3.13"           # optional, overrides the global setting
    pypiConstraints: ["protobuf<6"] # optional, like uv's constraint-dependencies
    pypiSeedLockfile: my-service/uv.lock # optional, versions preferred on the first resolution
```

The `resolutions` field of the island's workspaces applies to the island (in addition to the root workspace's), which is how uv's `override-dependencies` are expressed:

```json
{
  "name": "my-service",
  "dependencies": {"python-jose": "pypi:>=3.4,<3.5"},
  "resolutions": {"pyasn1": "pypi:>=0.6.3"}
}
```

The island's resolution is stored in `yarn.lock` along with a hash of its inputs; as long as they don't change, installs reuse the locked resolution without contacting the index.

## Platforms

Markers and wheels are evaluated for every platform listed in `supportedArchitectures` (plus the current one), with the island's Python version:

```yaml
supportedArchitectures:
  - {os: [darwin], cpu: [arm64]}
  - {os: [linux], cpu: [x64, arm64], libc: [glibc]}
```

- Requirements whose markers are false on every platform are dropped; those true on every platform are regular dependencies; the others keep their marker and are only installed where it evaluates to true.
- Releases shipping platform-specific wheels are locked with one variant per platform (`pkg@pypi:==1.0#platform=linux-x64-glibc`), each pinned to its wheel. Only the variants matching `supportedArchitectures` are downloaded, and the one matching the current machine is installed. A single lockfile therefore works on macOS and Linux.

Wheel compatibility follows `packaging.tags`: `cpXY`, `abi3`, `pyX-none-any`, `manylinux_2_N` / `musllinux_1_N` / `macosx_X_Y`. The newest platform versions wheels may require are set by `pypiManylinuxTarget` (default `2.34`) and `pypiMacosTarget` (default `14.0`).

## Source distributions

Packages only available as source distributions are built into wheels with [PEP 517](https://peps.python.org/pep-0517/) when the venv is created: Yarn creates an isolated environment with the island's interpreter, installs the `build-system.requires` from the same index with pip, and calls the backend's `build_wheel` hook. Built wheels are cached in the global folder, so each sdist is built once per machine and Python version.

## Interpreter

Each venv is created from an interpreter matching the island's `pythonVersion`:

- with `pythonPreference: managed` (default), a [python-build-standalone](https://github.com/astral-sh/python-build-standalone) build from the global folder, then one from the `PATH`, then a download (unless `pythonDownloads: false`);
- with `pythonPreference: system`, interpreters from the `PATH` are preferred.

`YARN_PYTHON_EXECUTABLE` forces a specific interpreter.

## Virtual environments

Each workspace of a venv island gets a standard virtual environment in `<workspace>/.venv` that works without Yarn - point your IDE, `pytest`, `mypy` or `ruff` at `.venv/bin/python` directly:

- `pyvenv.cfg`, `bin/python` (and `python3`, `python3.X`), `bin/activate`;
- `lib/python3.X/site-packages` with the installed wheels; console scripts from `entry_points.txt` in `bin/`;
- local workspaces (the workspace itself, and the workspaces it depends on through `workspace:`) installed in editable mode through a `.pth` file, along with their `[project.scripts]`.

Wheels are unpacked once in the global folder and cloned (copy-on-write, on APFS) or hardlinked into each venv, so many venvs sharing the same packages don't multiply the disk usage. Venvs are only rebuilt when their content changes. `yarn python` and the scripts of venv workspaces run with the venv activated.
