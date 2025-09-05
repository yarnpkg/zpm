# CLI commands

We use a Rust implementation of [Clipanion](http://github.com/arcanis/clipanion-rs/) to build our CLI commands, strongly inspired by the [TypeScript implementation](https://github.com/arcanis/clipanion/). It works roughly the same way but Rust allows a very nice improvements: option parameters can now be strongly typed.

## Command definition

Commands are defined using the `#[cli::command]` macro. This macro is used to define the command name, its category, its description, and its options.

```rust
#[cli::command]
#[cli::path("config")]
#[cli::category("Configuration commands")]
#[cli::description("List the project's configuration values")]
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
