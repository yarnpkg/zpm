# Running Tests

## Prerequisites

Before running tests, you must build the release binaries:

```bash
cargo build --release
```

## Running Integration Tests

Use the following command to run integration tests:

```bash
yarn test:integration <jest cli options>
```

For example:
- Run all tests: `yarn test:integration`
- Run a specific test file: `yarn test:integration path/to/test.ts`
- Run tests matching a pattern: `yarn test:integration -t "pattern"`
- Run in watch mode: `yarn test:integration --watch`

## Reading spawn logs

You can access the logs of any Yarn command spawned within a test by adding the `JEST_LOG_SPAWNS=1` environment variable.

## Creating temporary projects

You can setup temporary projects by:

1. Creating a new temporary folder
2. Adding an empty `package.json` file
3. Running `yarn switch link path/to/target/Release/yarn-bin` inside this temporary folder
4. Subsequent `yarn` commands should then use the local binary
