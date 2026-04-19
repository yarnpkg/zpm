---
category: concepts
slug: concepts/switch
title: Yarn Switch
description: A description of Yarn Switch, the official way to manage Yarn binaries across projects.
sidebar:
  order: 10
---

Yarn manages your dependency versions, but what about the Yarn version? That's the role of Yarn Switch.

Distributed as a separate binary with each Yarn release, Yarn Switch is a light binary that substitutes to the Yarn binaries and ensures that your team always uses the correct version of Yarn for the active project.

Here's what happens under the hood when you run a Yarn command:

1. Yarn Switch (`~/.yarn/switch/bin/yarn`) gets called.
2. It finds the nearest `package.json` file containing a `packageManager` field.
3. It checks whether that field references Yarn, and returns an error if not.
4. It then checks whether the requested version is available locally. If not, it downloads it.
5. It executes the cached binary, passing along any CLI arguments you provided.

:::note
Yarn Switch is very similar in idea to [Corepack](https://www.google.com/search?q=corepack&oq=corepack&gs_lcrp=EgZjaHJvbWUyCQgAEEUYORiABDIHCAEQABiABDIHCAIQABiABDIHCAMQABiABDIGCAQQRRhBMgYIBRBFGD0yBggGEEUYPDIGCAcQRRhB0gEIMTAzM2oxajSoAgCwAgE&sourceid=chrome&ie=UTF-8), except it's officially maintained by the Yarn team and is designed specifically for Yarn.

Given that the Node.js TSC [decided to phase out Corepack into Node.js](https://github.com/nodejs/TSC/pull/1697#issuecomment-2737093616), we decided to refocus our efforts, and now recommend using Yarn Switch over Corepack when possible.
:::

## Where are the binaries downloaded from?

Yarn Switch downloads the binaries from the official website. Your network administrators may need to allowlist the `repo.yarnpkg.com` domain for the endpoints to be available.

We don't offer proxy settings at the moment, but contributions to this effect are welcome.

## Configuring Yarn Switch for Docker images

As you won't want to rely on our endpoints for your runtime images, you should make sure to populate your images at build time with the Yarn version your project will need to run. You usually will face one of those two scenarios:

### You run your container as root

```docker
RUN curl -s https://repo.yarnpkg.com/install | bash
ENV PATH="/root/.yarn/switch/bin:$PATH"

# The --install flag makes sure your image contains
# the Yarn version used in your local project
RUN yarn switch cache --install
```

### Your runtime user is different from the build user

```docker
RUN curl -s https://repo.yarnpkg.com/install | bash
RUN mv ~/.yarn/switch/bin/yarn /usr/local/bin/yarn

USER node

# The --install flag makes sure your image contains
# the Yarn version used in your local project
RUN yarn switch cache --install
```

## Frequent questions

### How to upgrade to a new Yarn version?

Running `yarn set version latest` will make Yarn bump the `packageManager` field in your `package.json` file to the new release.

### Are the binaries signed?

The binaries aren't signed at the moment, but we're working on it and hope to have that set up before Yarn 6 reaches a stable release.
