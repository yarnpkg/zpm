extern crate zpm_allocator;

use std::process::ExitCode;

/// The commands run on a dedicated thread rather than on the main thread, so
/// that the stack they get doesn't depend on the platform (the main thread
/// gets 1 MiB on Windows, 8 MiB on most Linux and macOS systems, whatever
/// `ulimit -s` says otherwise). Some of the algorithms we use are recursive
/// (peer dependency resolution, for instance, goes up to 1000 levels deep),
/// so the stack they need grows with the depth of the dependency tree.
///
/// This is only an address space reservation; the memory is only committed
/// as it gets used.
const COMMAND_THREAD_STACK_SIZE: usize = 64 * 1024 * 1024;

fn main() -> ExitCode {
    let command_thread = std::thread::Builder::new()
        .name("main".to_string())
        .stack_size(COMMAND_THREAD_STACK_SIZE)
        .spawn(run)
        .expect("Failed to spawn the command thread");

    match command_thread.join() {
        Ok(exit_code)
            => exit_code,

        // The panic message has already been printed by the panic hook
        Err(panic)
            => std::panic::resume_unwind(panic),
    }
}

fn run() -> ExitCode {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed building the Runtime")
        .block_on(async_main())
}

async fn async_main() -> ExitCode {
    #[cfg(windows)]
    zpm_utils::windows_disable_std_handles_inheritance();

    env_logger::init();
    let otel_guard = zpm::otel::init();

    if std::env::var_os("NO_COLOR").is_some() {
        colored::control::set_override(false);
    } else if let Ok(force_color) = std::env::var("FORCE_COLOR") {
        colored::control::set_override(force_color != "0");
    }

    let exit_code = zpm::commands::run_default(None).await;

    if let Some(otel_guard) = otel_guard {
        otel_guard.shutdown();
    }

    exit_code
}
