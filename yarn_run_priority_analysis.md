# Yarn Run Priority Issue Analysis

## Issue Summary

The `yarn run` command in this implementation is **incorrectly preferring binaries over scripts**, which is the opposite of the documented and expected behavior.

## Expected Behavior (per Official Documentation)

According to the [official Yarn documentation](https://yarnpkg.com/cli/run), when `yarn run <scriptName>` is executed, the resolution order should be:

1. **Scripts first**: Check if the `scripts` field from the local package.json contains a matching script name
2. **Binaries second**: Only if no script is found, check if one of the local workspace's dependencies exposes a binary with a matching name
3. **Cross-workspace scripts**: If the name contains a colon and matches exactly one script across workspaces

## Current (Incorrect) Implementation

In `packages/zpm/src/commands/run.rs`, lines 39-65, the logic is:

```rust
let maybe_binary = project.find_binary(&self.name);

if let Ok(binary) = maybe_binary {
    // Execute binary immediately
    Ok(ScriptEnvironment::new()?
        .with_project(&project)
        .with_package(&project, &project.active_package()?)?
        .enable_shell_forwarding()
        .run_binary(&binary, &self.args)
        .await
        .into())
} else if let Err(Error::BinaryNotFound(_)) = maybe_binary {
    // Only try scripts if binary lookup failed
    let maybe_script = project.find_script(&self.name);
    // ... script execution logic
}
```

### Problem Analysis

1. **Binary lookup happens first** (`project.find_binary(&self.name)`)
2. **Script lookup only happens if binary lookup fails** with `Error::BinaryNotFound`
3. This means if both a script and a binary exist with the same name, the binary will always be executed instead of the script

## Correct Implementation Should Be

The logic should be reversed:

```rust
// Try scripts first
let maybe_script = project.find_script(&self.name);

if let Ok((locator, script)) = maybe_script {
    // Execute script
    Ok(ScriptEnvironment::new()?
        .with_project(&project)
        .with_package(&project, &locator)?
        .enable_shell_forwarding()
        .run_script(&script, &self.args)
        .await
        .into())
} else if let Err(Error::ScriptNotFound(_)) = maybe_script {
    // Only try binaries if script lookup failed
    let maybe_binary = project.find_binary(&self.name);
    // ... binary execution logic
}
```

## Impact

This bug means:
- Users expecting standard Yarn behavior will get unexpected results
- Scripts may be shadowed by binaries with the same name
- Behavior differs from official Yarn implementations (Classic and Modern)
- Could break existing workflows that depend on script precedence

## Files Affected

- `packages/zpm/src/commands/run.rs` (lines 39-65) - Main logic needs to be reversed
- The helper methods `find_script` and `find_binary` in `packages/zpm/src/project.rs` appear to be working correctly

## Recommendation

The fix is straightforward: reverse the order of the lookup logic in the `Run::execute` method to check scripts before binaries, matching the official Yarn specification.