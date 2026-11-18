# CLI commands

All commands are located within the [packages/zpm/src/commands](../../packages/zpm/src/commands/) folder.

We use a Rust implementation of [Clipanion](http://github.com/arcanis/clipanion-rs/) to build our CLI commands, strongly inspired by the [TypeScript implementation](https://github.com/arcanis/clipanion/). It works roughly the same way but Rust allows a very nice improvements: option parameters can now be strongly typed.

## Command definition

Commands are defined using the `#[cli::command]` macro. This macro is used to define the command name, its category, its description, and its options.

```rust
#[cli::command]
#[cli::path("config")]
#[cli::category("Configuration commands")]
#[cli::description("List the project's configuration values")]
```

The first line of the doc comment is the command description, and the following paragraphs are its details. Clipanion joins consecutive doc comment lines into a single paragraph, so separate paragraphs (and list items) with an empty `///` line.

## Command examples

Examples are written as the last section of the doc comment: a `## Examples` paragraph followed by pairs of paragraphs, a short description and then the command wrapped in single backticks. clipanion-rs takes this section out of the details when the command is compiled (the build fails if a description has no command, or if a command isn't a single code span) and exposes it as the `examples` field of the command spec. `yarn <command> --help` prints them in its "Examples" section, and the website renders them on the command page (see [website/src/utils/cli.ts](../../website/src/utils/cli.ts)). Only document commands and flags you ran against your build.

```rust
/// Add new dependencies to the project
///
/// This command adds packages to the nearest workspace manifest and then runs an install.
///
/// ## Examples
///
/// Add a regular package to the current workspace
///
/// `yarn add lodash`
///
/// Add a package to the `devDependencies` of the current workspace
///
/// `yarn add -D typescript`
///
#[cli::command]
#[cli::path("add")]
```

## Command execution

Commands are executed using the `execute` method. This method is used to execute the command and return a result. It can be async:

```rust
impl Config {
    #[tokio::main]
    pub async fn execute(&self) -> Result<(), Error> {
        Ok(())
    }
}

> [!NOTE]
> At the moment the tokio::main attribute must be added to async commands, but since it prevents calling commands from other commands we should change that to instead turn ALL commands into async ones and use the `program_async!` macro instead of `program!` in [commands/mod.rs](../../packages/zpm/src/commands/mod.rs).

## Debugging the CLI parsing

You can set the `CLIPANION_DEBUG=1` environment variable to see the states the parser traverses before selecting the final CLI command.
