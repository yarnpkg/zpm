---
category: advanced
slug: advanced/contributing
title: Contributing
description: Yarn's contributing guide.
---

Thanks for being here! Yarn gives a lot of importance to being a community project, and we rely on your help as much as you rely on ours. In order to help you help us, we've invested in an infra and documentation that should make contributing to Yarn very easy. If you have any feedback on what we could improve, please open an issue to discuss it!

## Opening an issue

Issues have to follow the issue template. Make sure to follow everything, with a special attention for the reproduction. If we can't reproduce your problem, we won't solve it.

## How can you help?

- Review our documentation! We often aren't native english speakers, and our grammar might be a bit off. Any help we can get that makes our documentation more digestible is appreciated!

- Talk about Yarn in your local meetups! Even our users aren't always aware of some of our features. Learn, then share your knowledge with your own circles!

- Help with our infra! There are always small improvements to do: run tests faster, uniformize the test names, improve the way our version numbers are setup, ...

- Write code! We have so many features we want to implement, and so little time to actually do it... Any help you can afford will be appreciated, and you will have the satisfaction to know that your work helped literally millions of developers!

## Finding work to do

It might be difficult to know where to start on a fresh codebase. To help a bit with this, we try to mark various issues with tags meant to highlight issues that we think don't require as much context as others:

- [Good First Issue](https://github.com/yarnpkg/berry/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22) are typically self-contained features of a limited scope that are a good way to get some insight as to how Yarn works under the hood.

- [Help Wanted](https://github.com/yarnpkg/berry/issues?q=is%3Aissue+is%3Aopen+label%3A%22help+wanted%22) are issues that don't require a lot of context but also have less impact than the ones who do, so no core maintainer has the bandwidth to work on them.

Finally, feel free to pop on our [Discord channel](https://discordapp.com/invite/yarnpkg) to ask for help and guidance. We're always happy to see new blood, and will help you our best to make your first open-source contribution a success!

## Testing your code

Most of the CLI is written in Rust, so build the binaries before running integration tests:

```bash
cargo build -r
```

The CLI integration tests are Jest tests backed by the freshly built binary:

```bash
yarn test:integration <jest options>
```

You can filter the tests you want to run by passing normal Jest options:

```bash
yarn test:integration commands/install.test.ts
yarn test:integration -t 'it should correctly install a single dependency that contains no sub-dependencies'
```

The ecosystem e2e tests are shell scenarios:

```bash
yarn test:e2e <e2e test name, minus the .sh>
```

Should you need to write a test, the integration specs are located in `tests/acceptance-tests/pkg-tests-specs/sources`. The `makeTemporaryEnv` utility generates a temporary project; the first parameter becomes `package.json`, the second becomes `.yarnrc.yml`, and the callback runs inside that project.

## Checking Constraints

We use [constraints](/concepts/constraints) to enforce various rules across the repository. They are declared from `yarn.config.ts`, `yarn.config.mjs`, or `yarn.config.cjs` files using the JavaScript constraints API.

Constraints can be checked with `yarn constraints`, and fixed with `yarn constraints --fix`. Generally speaking:

- Workspaces must not depend on conflicting ranges of dependencies.

- Workspaces must not depend on non-workspace ranges of available workspaces.

- Repository-specific workspace rules should be expressed through the JavaScript constraints API.

- Workspaces must point our repository through the `repository` field.

## Preparing your PR to be released

In order to track which packages need to be released, we use deferred version files. When a package needs a release, run the appropriate version command with `--deferred`, for example `yarn version patch --deferred`, `yarn version minor --deferred`, or `yarn version decline --deferred`.

You can check if you've set everything correctly with `yarn version check`.

If you expect a package to have to be released again but Yarn doesn't offer you this choice, first check whether the name of your local branch is `master`. If that's the case, Yarn might not be able to detect your changes (since it will do it against `master`, which is yourself). Run the following commands:

```bash
git checkout -b my-feature
git checkout -
git reset --hard upstream/master
git checkout -
yarn version check
```

If it fails and you have no idea why, feel free to ping a maintainer and we'll do our best to help you.

## Reviewing other PRs

You're welcome to leave comments if you spot glaring bugs, but do not approve PRs if you're not a member.
It's generally seen as [bad form](https://twitter.com/brian_d_vaughn/status/1224051534536667137) in the open source community.

## Writing documentation

We use Astro to generate HTML pages from Markdown sources.

Our website is stored within the `website` directory. You can change a page by modifying the corresponding Markdown file in `website/src/docs`, or the corresponding Astro route in `website/src/pages`.

Then run the following command to spawn a local server and see your changes:

```bash
yarn --cwd website dev
```

Before opening a PR, make sure the website still builds:

```bash
yarn --cwd website build
```

## Profiling

Build a release binary before profiling:

```bash
cargo build -r
```

Then run your profiler against the generated binary at `target/release/yarn-bin`.

```bash
target/release/yarn-bin install
```
