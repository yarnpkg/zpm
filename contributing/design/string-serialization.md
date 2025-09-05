# String serialization

Rust has some native traits to turn elements into strings. The main one is `Display`, which is automatically called when using the `{}` placeholder in a `println!` or `format!` macro. Implementing `Display` also automatically derives a `ToString` implementation. **Don't use it.**

One problem with this approach is that there are multiple reasons why we would want to print something on disk, depending on who's consuming it:

- We're printing it on the terminal for a human reader.
- We're printing it as a serialized JSON object.
- We're printing it as a serialized string.

While the two last cases look similar I see them as semantically different enough that they should have different traits. For instance an object could have both a string representation used when parsing a object from a command line argument, and a more JSON-friendly representation when storing said object in a file (for example numbers, which should be clean JSON numbers once serialized).

To try to formalize this, the codebase has three traits:

- `ToHumanString` is meant to be used when printing things on the screen. You should use appropriate colorization and formatting to make it more readable. It's not a bidirectional trait, you can't parse a string into a `ToHumanString` object, so feel free to remove information that wouldn't be useful for a human reader (as an example we only print the N first characters of hashes).

> [!NOTE]
> `ToHumanString` is badly named because the function it contains is called `to_print_string`. I need to fix that.

- `ToFileString` and `FromFileString` are bidirectional traits meant to be used when serializing data structures to and from strings, for example so they can be passed as command line arguments. Such representations should be lossless, meaning that if you serialize an object and then deserialize it, you should get the same object back.

## Relation with native traits

The `Display` trait is never used in the codebase. Its cardinal sin is to be automatically called when using the `{}` placeholder in a `println!` or `format!` macro. Due to that, errors are very easy to make.

> [!NOTE]
> It'd be ideal if Rust allowed us to write our own format macros that would call our own trait rather than `Display`, so we could have a `format_print!` macro. But unfortunately that's not possible at this time.

The `FromStr` trait is used because it interacts with third-party libraries, Clipanion in particular. It's not implemented by default, you should use the following macro on each type implementing `FromFileString`:

```rust
impl_file_string_from_str!(MyType);
```

> [!NOTE]
> Should we automatically implement `FromStr` for all types implementing `FromFileString`? Rust really doesn't like overlapping generic implementation, so I think something like `impl<T: FromFileString> FromStr for T` would not work.

## Relation with Serde

Serde is the main serialization framework in the Rust ecosystem. We use it to describe how to serialize and deserialize many data structures.

While implementing `ToFileString` doesn't automatically implement `Serialize`, you can use the following macro to do so:

```rust
impl_file_string_serialization!(MyType);
```

This macro implements the `Serialize` trait for the type, and adds a `deserialize` implementation for each type implementing `FromFileString`.
