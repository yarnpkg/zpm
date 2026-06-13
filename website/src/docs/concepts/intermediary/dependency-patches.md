---
category: concepts
slug: concepts/patches
title: Dependency patches
description: How to fix your dependencies without having to fork them entirely while waiting for an update.
sidebar:
  order: 2
---

It sometimes happen that you need to make small changes to a dependency, just to workaround some small issue. The recommended action is to make a PR upstream, but it may take time until your changes get through review and end up in a release; what to do in the meantime? You have two options:

- You can use the `git:` protocol, which will let you install a project straight from its development repository, provided it was correctly setup.

- Or you can use the `patch:` protocol to make small changes to the dependencies straight from your project, while keeping them separated from the original code.

No more waiting around for pull requests to be merged and published, no more forking repos just to fix that one tiny thing preventing your app from working: the builtin patch mechanism will always let you unblock yourself.

## Making patches

To create a patch, run the `yarn patch` command and pass it a package name. Yarn will extract the requested package to a temporary folder which you're then free to edit as you wish.

Once you're done with your changes, all that remains is to run `yarn patch-commit -s` and pass it the path to the temporary folder Yarn generated: a patch file will be generated in `.yarn/patches` and applied to your project. Commit it, and you're set to go.

## Maintaining patches

By default, `yarn patch` will always reset the patch. If you wish to add new changes, use the `yarn patch ! --update` flag and follow the same procedure as before - your patch will be regenerated.

## Limitations

- Patches are computed at fetch time rather than resolution time, so package dependencies have already been extracted by the time Yarn reads your patched files. Prefer the `packageExtensions` mechanism to add new dependencies to a package.

- Patches are ill-suited for modifying binary files. Minified files are problematic as well, although we would welcome a PR improving the feature to automatically process such files through a file formatter.
