---
category: concepts
slug: concepts/protocols
title: Dependency protocols
description: The various options you have to define dependencies in your application.
sidebar:
  order: 3
---

Yarn supports various protocols for defining dependencies in your application. While you're certainly familiar with the semver protocol which downloads packages from the npm registry, Yarn is also able to retrieve packages from git, the filesystem, or even generate them on the fly.

## Available protocols

:::note
Some of these protocols have alternative syntaxes, such as the git protocol which also supports strings like `<username>/<reponame>`. Check their respective pages for more information.
:::

<div class="[&_table]:table-fixed [&_th:first-child]:w-[200px]">

| Protocol | Description |
| --- | --- |
| `catalog:` | Delegate the dependency to the project configuration. |
| `exec:` | Generate a package on the fly via a script. |
| `file:` | Compile a local folder or extract a tgz archive. |
| `git:` | Retrieve a package from a git repository. |
| `jsr:` | Download a package from the JSR registry. |
| `link:` | Pretend the specified folder is a package. |
| `npm:` | Download a package from the npm registry. |
| `patch:` | Apply a patch to an existing package. |
| `portal:` | Connect the project to a package in another folder. |
| `pypi:` | Download a package from the Python Package Index. |
| `workspace:` | Connect a workspace to another workspace. |

</div>
