# General commands

## Compiling the project

I've been working with the release binary even in development, and tests are configured to use it as well so we can have performances similar to what users would experience. Note that the release builds are configured to contain debug information, even if optimized.

```bash
cargo build -p zpm -r
```

## Running integration tests

The ZPM codebase can run all tests from the Yarn Berry repository. To do that, clone github.com/yarnpkg/berry inside your home folder then run the following command from the ZPM repository:

```bash
yarn berry test:integration {jest options}
```

The integration tests can sometimes fail due to being overly dependent on the Yarn Berry implementation. Some examples:

- Asserting an exact error message, when ZPM may not print the exact same one.
- Using snapshots or assertions on the lockfile content.

In such cases you may need to tweak the test in the Berry repository to make it more generic and support both codebases.

> [!NOTE]
> The integration tests aren't run automatically on every PR as they take ~20mns to run and we pay for the CI minutes as long as the repository is private. Once it's made public we'll change that to run them automatically.
>
> In the meantime you can run them manually by going to the [workflow page](https://github.com/yarnpkg/zpm/actions/workflows/tests.yml) and clicking "Run workflow" with the relevant branch selected.

## Running unit tests

Unit tests are run using the standard `cargo test` command. We try not to rely too much on unit tests as they are prone to break should we swap their API shape, but in some cases, like when working on internal libraries, it is useful to have them.

You can filter the tests to run by using the `-p` flag to only run tests from a specific packagea:

```bash
cargo test -p zpm-parsers
```

## Extracting flamegraphs

To find performance bottlenecks I suggest using the [Cargo Flamegraph crate](https://github.com/flamegraph-rs/flamegraph). It can be installed with:

```
cargo install flamegraph
```

Then, to extract a flamegraph, run:

```bash
cargo flamegraph --root -r -p zpm
```

This will perform an install in the ZPM repository itself, and generate a `flamegraph.svg` file. You can then open it in your browser to see a flamegraph similar to this one:

![Flamegraph](./flamegraph.svg)
