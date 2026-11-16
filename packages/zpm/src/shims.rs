//! Helpers to generate the small wrapper scripts we put on the `PATH` of the
//! processes we spawn (and in `node_modules/.bin`), along with the logic
//! needed to run them on Windows, where shebangs and executable bits don't
//! exist.
//!
//! The shim generators are always compiled so that they can be tested on
//! every platform.

use std::io::Read;

use zpm_utils::{Path, ToFileString};

/// Escapes an argument so that it can be passed through a line of a `.cmd`
/// file. All the cmd metacharacters (including quotes) are escaped with a
/// caret so that cmd never enters its quoted mode, and the argument is then
/// quoted according to the MSVC runtime rules, which is how the spawned
/// program will split its command line.
///
/// ```
/// use zpm::shims::cmd_escape_arg;
///
/// assert_eq!(cmd_escape_arg("foo"), r#"^"foo^""#);
/// assert_eq!(cmd_escape_arg("a b&c"), r#"^"a^ b^&c^""#);
/// assert_eq!(cmd_escape_arg(r#"say "hi""#), r#"^"say^ \^"hi\^"^""#);
/// assert_eq!(cmd_escape_arg(r#"a\"b"#), r#"^"a\\\^"b^""#);
/// assert_eq!(cmd_escape_arg(r"C:\dir\"), r#"^"C:\dir\\^""#);
/// assert_eq!(cmd_escape_arg("100%"), r#"^"100%%^""#);
/// ```
pub fn cmd_escape_arg(arg: &str) -> String {
    let mut quoted
        = String::with_capacity(arg.len() + 2);

    quoted.push('"');

    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => {
                backslashes += 1;
            },

            '"' => {
                // The preceding backslashes were already pushed once; they
                // must be doubled, plus one more to escape the quote itself
                quoted.extend(std::iter::repeat('\\').take(backslashes + 1));
                quoted.push('"');
                backslashes = 0;
                continue;
            },

            _ => {
                backslashes = 0;
            },
        }

        quoted.push(c);
    }

    // Backslashes preceding the closing quote must be doubled
    quoted.extend(std::iter::repeat('\\').take(backslashes));
    quoted.push('"');

    let mut escaped
        = String::with_capacity(quoted.len() * 2);

    for c in quoted.chars() {
        match c {
            '%' => escaped.push_str("%%"),
            '(' | ')' | '[' | ']' | '!' | '^' | '"' | '`' | '<' | '>' | '&' | '|' | ';' | ',' | ' ' | '*' | '?' => {
                escaped.push('^');
                escaped.push(c);
            },
            _ => escaped.push(c),
        }
    }

    escaped
}

/// Quotes an argument for a program built on the MSYS / Cygwin runtime
/// (such as the bash shipped with Git for Windows). Their command line
/// parser doesn't follow the MSVC rules used by Rust when spawning
/// processes: inside double quotes, `\\` is an escaped backslash, so
/// arguments containing backslash pairs would otherwise get corrupted.
///
/// ```
/// use zpm::shims::msys_quote_arg;
///
/// assert_eq!(msys_quote_arg("foo"), r#""foo""#);
/// assert_eq!(msys_quote_arg(""), r#""""#);
/// assert_eq!(msys_quote_arg(r"replace(/\\/g)"), r#""replace(/\\\\/g)""#);
/// assert_eq!(msys_quote_arg(r#"say "hi""#), r#""say \"hi\"""#);
/// ```
pub fn msys_quote_arg(arg: &str) -> String {
    let mut quoted
        = String::with_capacity(arg.len() + 2);

    quoted.push('"');

    for c in arg.chars() {
        if c == '\\' || c == '"' {
            quoted.push('\\');
        }

        quoted.push(c);
    }

    quoted.push('"');
    quoted
}

/// Whether the program is a POSIX shell (in practice the MSYS bash we use to
/// run scripts on Windows, cf `msys_quote_arg`).
pub fn is_posix_shell(program: &std::path::Path) -> bool {
    program.file_stem()
        .map(|stem| stem.eq_ignore_ascii_case("bash") || stem.eq_ignore_ascii_case("sh"))
        .unwrap_or(false)
}

/// Escapes an argument for a POSIX shell.
pub fn sh_escape_arg(arg: &str) -> String {
    format!("'{}'", arg.replace("'", "'\"'\"'"))
}

/// Generates a `.cmd` shim running `argv0` with the given arguments,
/// followed by the arguments the shim itself received.
///
/// The leading `goto` is a trick preventing cmd from asking "Terminate
/// batch job (Y/N)?" when the user presses Ctrl-C.
pub fn make_cmd_shim(argv0: &str, args: &[String]) -> String {
    let argv0
        = argv0.replace('/', "\\").replace('%', "%%");

    let mut line
        = format!(r#"@goto #_undefined_# 2>NUL || @title %COMSPEC% & @setlocal & @"{}""#, argv0);

    for arg in args {
        line.push(' ');
        line.push_str(&cmd_escape_arg(arg));
    }

    line.push_str(" %*\r\n");
    line
}

/// Generates a POSIX shell shim running `argv0` with the given arguments,
/// followed by the arguments the shim itself received.
pub fn make_sh_shim(argv0: &str, args: &[String]) -> String {
    let escaped_args = args.iter()
        .map(|arg| format!(" {}", sh_escape_arg(arg)))
        .collect::<String>();

    format!("#!/bin/sh\nexec \"{}\"{} \"$@\"\n", argv0, escaped_args)
}

/// Whether a word can be written as-is in both the `.cmd` and the POSIX
/// shell shims. Interpreter names and the flags of a shebang almost always
/// qualify, which keeps the shims as readable as cmd-shim's (which pastes
/// them verbatim); anything else gets quoted.
fn is_shim_safe_word(word: &str) -> bool {
    !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '=' | '+' | '@'))
}

/// What should be executed to run a script located inside a
/// `node_modules/.bin` folder on Windows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelativeShimTarget {
    /// The target must be run through the given interpreter, with the
    /// given arguments inserted before the target path.
    Interpreter(String, Vec<String>),
    /// The target can be executed directly.
    Direct,
}

impl RelativeShimTarget {
    /// Figures out how to run the target the way `@zkochan/cmd-shim` (used
    /// by npm, pnpm, and Berry) does: through the interpreter of its
    /// shebang (as returned by `parse_shebang`) or, when it has none, the
    /// interpreter associated with its extension. Targets with an unknown
    /// extension are executed directly.
    ///
    /// ```
    /// use zpm::shims::RelativeShimTarget;
    /// use zpm_utils::Path;
    ///
    /// let shebang = Some(("node".to_string(), vec!["--no-warnings".to_string()]));
    ///
    /// assert_eq!(RelativeShimTarget::new(&Path::try_from("../pkg/cli").unwrap(), shebang), RelativeShimTarget::Interpreter("node".to_string(), vec!["--no-warnings".to_string()]));
    /// assert_eq!(RelativeShimTarget::new(&Path::try_from("../pkg/cli.js").unwrap(), None), RelativeShimTarget::Interpreter("node".to_string(), vec![]));
    /// assert_eq!(RelativeShimTarget::new(&Path::try_from("../pkg/cli.bat").unwrap(), None), RelativeShimTarget::Interpreter("cmd".to_string(), vec!["/C".to_string()]));
    /// assert_eq!(RelativeShimTarget::new(&Path::try_from("../pkg/cli.exe").unwrap(), None), RelativeShimTarget::Direct);
    /// ```
    pub fn new(target_path: &Path, shebang: Option<(String, Vec<String>)>) -> Self {
        if let Some((program, args)) = shebang {
            return Self::Interpreter(program, args);
        }

        let extension
            = target_path.extname().map(|ext| ext.to_ascii_lowercase());

        let (program, args): (&str, &[&str]) = match extension.as_deref() {
            Some(".js" | ".cjs" | ".mjs")
                => ("node", &[]),

            // cmd requires the `/C` flag to execute batch files
            Some(".cmd" | ".bat")
                => ("cmd", &["/C"]),

            Some(".ps1")
                => ("pwsh", &[]),

            Some(".sh")
                => ("sh", &[]),

            _ => {
                return Self::Direct;
            },
        };

        Self::Interpreter(program.to_string(), args.iter().map(|arg| arg.to_string()).collect())
    }
}

/// Generates the `.cmd` shim for a binary located at `target_rel_path`
/// (relative to the directory containing the shim).
pub fn make_relative_cmd_shim(target_rel_path: &Path, target: &RelativeShimTarget) -> String {
    let target_expr
        = format!(r#""%~dp0\{}""#, target_rel_path.to_file_string().replace('/', "\\").replace('%', "%%"));

    let invocation = match target {
        RelativeShimTarget::Interpreter(program, args) => {
            let mut line = match is_shim_safe_word(program) {
                true => format!("@{}", program),
                false => format!(r#"@"{}""#, program.replace('/', "\\").replace('%', "%%")),
            };

            for arg in args {
                line.push(' ');

                match is_shim_safe_word(arg) {
                    true => line.push_str(arg),
                    false => line.push_str(&cmd_escape_arg(arg)),
                }
            }

            line.push(' ');
            line.push_str(&target_expr);
            line
        },

        RelativeShimTarget::Direct => {
            format!("@{}", target_expr)
        },
    };

    format!("@goto #_undefined_# 2>NUL || @title %COMSPEC% & @setlocal & {} %*\r\n", invocation)
}

/// Generates the POSIX shell shim (used by Git Bash and similar shells) for
/// a binary located at `target_rel_path` (relative to the directory
/// containing the shim).
///
/// Under MSYS2, MinGW, and Cygwin (which includes Git Bash), `$0` is a
/// POSIX path (`/c/proj/...`) when the shim is reached through the `PATH`
/// or an absolute path. Native Windows programs such as `node.exe` can't
/// read it when the shell's argument conversion is disabled, so we turn
/// the shim's folder into a Windows path with `cygpath` first, like
/// cmd-shim does.
pub fn make_relative_sh_shim(target_rel_path: &Path, target: &RelativeShimTarget) -> String {
    let target_expr
        = format!("\"$basedir/{}\"", target_rel_path.to_file_string());

    let invocation = match target {
        RelativeShimTarget::Interpreter(program, args) => {
            let mut line = match is_shim_safe_word(program) {
                true => program.clone(),
                false => sh_escape_arg(program),
            };

            for arg in args {
                line.push(' ');

                match is_shim_safe_word(arg) {
                    true => line.push_str(arg),
                    false => line.push_str(&sh_escape_arg(arg)),
                }
            }

            line.push(' ');
            line.push_str(&target_expr);
            line
        },

        RelativeShimTarget::Direct => {
            target_expr
        },
    };

    // The header is the same as cmd-shim's, byte for byte
    format!(concat!(
        "#!/bin/sh\n",
        "basedir=$(dirname \"$(echo \"$0\" | sed -e 's,\\\\,/,g')\")\n",
        "\n",
        "case `uname` in\n",
        "    *CYGWIN*|*MINGW*|*MSYS*)\n",
        "        if command -v cygpath > /dev/null 2>&1; then\n",
        "            basedir=`cygpath -w \"$basedir\"`\n",
        "        fi\n",
        "    ;;\n",
        "esac\n",
        "\n",
        "exec {} \"$@\"\n",
    ), invocation)
}

/// Extracts the interpreter and its arguments from a shebang line. The
/// interpreter is returned as a bare name (`#!/usr/bin/env python3` and
/// `#!/usr/bin/python3` both yield `python3`), since the absolute paths
/// used in shebangs don't exist on Windows.
///
/// ```
/// use zpm::shims::parse_shebang;
///
/// assert_eq!(parse_shebang(b"#!/bin/sh\necho hi"), Some(("sh".to_string(), vec![])));
/// assert_eq!(parse_shebang(b"#!/usr/bin/env python3 -u\n"), Some(("python3".to_string(), vec!["-u".to_string()])));
/// assert_eq!(parse_shebang(b"#!/usr/bin/env -S node --no-warnings\n"), Some(("node".to_string(), vec!["--no-warnings".to_string()])));
/// assert_eq!(parse_shebang(b"#!/usr/bin/env -Snode --no-warnings\n"), Some(("node".to_string(), vec!["--no-warnings".to_string()])));
/// assert_eq!(parse_shebang(b"#!/usr/bin/env bash\r\n"), Some(("bash".to_string(), vec![])));
/// assert_eq!(parse_shebang(b"console.log(42)"), None);
/// ```
pub fn parse_shebang(data: &[u8]) -> Option<(String, Vec<String>)> {
    let line = data.strip_prefix(b"#!")?;

    let line = line.iter()
        .position(|&b| b == b'\n')
        .map_or(line, |pos| &line[..pos]);

    let line
        = std::str::from_utf8(line).ok()?;

    let mut tokens
        = line.split_whitespace();

    let mut program
        = tokens.next()?;

    if program.ends_with("/env") || program == "env" {
        program = tokens.next()?;

        if program == "-S" {
            program = tokens.next()?;
        } else if let Some(split_program) = program.strip_prefix("-S") {
            program = split_program;
        }
    }

    let program
        = program.rsplit(['/', '\\']).next().unwrap_or(program);

    if program.is_empty() {
        return None;
    }

    Some((program.to_string(), tokens.map(str::to_string).collect()))
}

/// Reads the shebang of the given file, if any.
pub fn read_shebang(path: &Path) -> Option<(String, Vec<String>)> {
    let file
        = std::fs::File::open(path.to_path_buf()).ok()?;

    let mut buf
        = Vec::with_capacity(256);

    file.take(256).read_to_end(&mut buf).ok()?;

    parse_shebang(&buf)
}

/// Whether Windows can execute the given file without an interpreter.
pub fn has_windows_executable_extension(path: &Path) -> bool {
    path.extname()
        .map(|ext| matches!(ext.to_ascii_lowercase().as_str(), ".exe" | ".com" | ".cmd" | ".bat"))
        .unwrap_or(false)
}

/// Returns the interpreter that should be used to run the given file on
/// Windows, which doesn't support shebangs. Returns `None` when the file
/// can be executed directly (or when we can't tell).
#[cfg(windows)]
pub fn get_windows_interpreter(path: &Path) -> Option<(String, Vec<String>)> {
    if has_windows_executable_extension(path) {
        return None;
    }

    let (program, args)
        = read_shebang(path)?;

    let program = match program.as_str() {
        "sh" | "bash" => windows::find_bash()
            .map(|bash| bash.to_file_string())
            .unwrap_or(program),

        _ => program,
    };

    Some((program, args))
}

#[cfg(windows)]
pub mod windows {
    use std::{path::PathBuf, sync::LazyLock};

    use zpm_utils::Path;

    static BASH_PATH: LazyLock<Option<Path>> = LazyLock::new(|| {
        locate_bash().and_then(|path| Path::try_from(path).ok())
    });

    /// Finds the bash binary used to run scripts on Windows. We look for it
    /// in the `PATH` first, then next to the Git for Windows installation,
    /// since it ships with a full bash environment. The `bash.exe` provided
    /// by Windows in System32 is ignored, as it runs the scripts inside WSL
    /// rather than on the host.
    pub fn find_bash() -> Option<Path> {
        BASH_PATH.clone()
    }

    fn is_wsl_launcher(candidate: &std::path::Path) -> bool {
        let candidate
            = candidate.to_string_lossy().to_ascii_lowercase();

        let system_root
            = std::env::var("SystemRoot")
                .unwrap_or_else(|_| "C:\\Windows".to_string())
                .to_ascii_lowercase();

        candidate.starts_with(&format!("{}\\system32\\", system_root))
            || candidate.starts_with(&format!("{}\\sysnative\\", system_root))
            || candidate.contains("\\windowsapps\\")
    }

    fn locate_bash() -> Option<PathBuf> {
        let path_dirs = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
            .unwrap_or_default();

        for dir in &path_dirs {
            let candidate
                = dir.join("bash.exe");

            if candidate.is_file() && !is_wsl_launcher(&candidate) {
                return Some(candidate);
            }
        }

        // Git for Windows only adds its `cmd` folder to the PATH by default;
        // bash lives in its `bin` folder (`Git\cmd\git.exe` → `Git\bin\bash.exe`)
        for dir in &path_dirs {
            if !dir.join("git.exe").is_file() {
                continue;
            }

            for ancestor in dir.ancestors().skip(1).take(3) {
                let candidate
                    = ancestor.join("bin").join("bash.exe");

                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }

        let install_roots = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"]
            .iter()
            .filter_map(|var| std::env::var_os(var).map(PathBuf::from))
            .chain(std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Programs")));

        for root in install_roots {
            let candidate
                = root.join("Git").join("bin").join("bash.exe");

            if candidate.is_file() {
                return Some(candidate);
            }
        }

        None
    }

    /// Resolves a bare program name the same way `CreateProcess` callers
    /// such as cmd do, by looking for it in each `PATH` folder with each of
    /// the `PATHEXT` extensions. Rust's own lookup only ever tries `.exe`,
    /// which misses the `.cmd` wrappers used by npm, pnpm, and our shims.
    pub fn resolve_program(program: &str, path_env: &str) -> Option<PathBuf> {
        if program.contains(['/', '\\']) {
            return None;
        }

        let extensions = std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .filter(|ext| !ext.is_empty())
            .map(|ext| ext.to_ascii_lowercase())
            .collect::<Vec<_>>();

        let has_extension = std::path::Path::new(program)
            .extension()
            .map(|ext| extensions.contains(&format!(".{}", ext.to_string_lossy().to_ascii_lowercase())))
            .unwrap_or(false);

        for dir in std::env::split_paths(path_env) {
            if has_extension {
                let candidate
                    = dir.join(program);

                if candidate.is_file() {
                    return Some(candidate);
                }

                continue;
            }

            for ext in &extensions {
                let candidate
                    = dir.join(format!("{}{}", program, ext));

                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use zpm_utils::Path;

    use super::*;

    #[test]
    fn generates_cmd_shims() {
        assert_eq!(
            make_cmd_shim("C:/Program Files/nodejs/node.exe", &["C:/proj/bin.js".to_string()]),
            "@goto #_undefined_# 2>NUL || @title %COMSPEC% & @setlocal & @\"C:\\Program Files\\nodejs\\node.exe\" ^\"C:/proj/bin.js^\" %*\r\n",
        );
    }

    #[test]
    fn generates_sh_shims() {
        assert_eq!(
            make_sh_shim("node", &["/proj/it's.js".to_string()]),
            "#!/bin/sh\nexec \"node\" '/proj/it'\"'\"'s.js' \"$@\"\n",
        );
    }

    const RELATIVE_CMD_SHIM_PREFIX: &str
        = "@goto #_undefined_# 2>NUL || @title %COMSPEC% & @setlocal & ";

    const RELATIVE_SH_SHIM_PREFIX: &str = concat!(
        "#!/bin/sh\n",
        "basedir=$(dirname \"$(echo \"$0\" | sed -e 's,\\\\,/,g')\")\n",
        "\n",
        "case `uname` in\n",
        "    *CYGWIN*|*MINGW*|*MSYS*)\n",
        "        if command -v cygpath > /dev/null 2>&1; then\n",
        "            basedir=`cygpath -w \"$basedir\"`\n",
        "        fi\n",
        "    ;;\n",
        "esac\n",
        "\n",
    );

    // The shims generated for a `node_modules/.bin/cli` entry pointing to a
    // file with the given content: (target, content, `.cmd` shim minus its
    // prefix, sh shim minus its prefix).
    //
    // They were derived from the output of `@zkochan/cmd-shim@7.0.0` (the
    // version Berry uses) for the same files. The sh prefix is identical to
    // cmd-shim's; the commands run the same programs with the same arguments,
    // with these differences:
    //
    // - the `.cmd` shims are a single line using the Ctrl-C trick described
    //   in `make_cmd_shim`, without cmd-shim's `IF EXIST "%~dp0\node.exe"`
    //   branch (and its sh counterpart, `if [ -x "$basedir/node" ]`);
    // - absolute interpreter paths are reduced to their basename (cmd-shim
    //   keeps `/bin/sh` and `/usr/bin/node`, which don't exist on Windows);
    // - the sh shims always `exec`, and runs of spaces are collapsed.
    const RELATIVE_SHIM_CASES: &[(&str, &str, &str, &str)] = &[
        // env-node
        ("../pkg/cli.js", "#!/usr/bin/env node\nconsole.log(42);\n",
            "@node \"%~dp0\\..\\pkg\\cli.js\" %*\r\n",
            "exec node \"$basedir/../pkg/cli.js\" \"$@\"\n"),

        // env-s-node
        ("../pkg/cli.js", "#!/usr/bin/env -S node --enable-source-maps\nconsole.log(42);\n",
            "@node --enable-source-maps \"%~dp0\\..\\pkg\\cli.js\" %*\r\n",
            "exec node --enable-source-maps \"$basedir/../pkg/cli.js\" \"$@\"\n"),

        // env-s-node-ts
        ("../pkg/cli.ts", "#!/usr/bin/env -S node --experimental-strip-types --no-warnings\nconsole.log(42);\n",
            "@node --experimental-strip-types --no-warnings \"%~dp0\\..\\pkg\\cli.ts\" %*\r\n",
            "exec node --experimental-strip-types --no-warnings \"$basedir/../pkg/cli.ts\" \"$@\"\n"),

        // env-node-crlf
        ("../pkg/cli.js", "#!/usr/bin/env node\r\nconsole.log(42);\r\n",
            "@node \"%~dp0\\..\\pkg\\cli.js\" %*\r\n",
            "exec node \"$basedir/../pkg/cli.js\" \"$@\"\n"),

        // env-node-args
        ("../pkg/cli.js", "#!/usr/bin/env node --no-warnings\nconsole.log(42);\n",
            "@node --no-warnings \"%~dp0\\..\\pkg\\cli.js\" %*\r\n",
            "exec node --no-warnings \"$basedir/../pkg/cli.js\" \"$@\"\n"),

        // env-s-attached
        ("../pkg/cli.js", "#!/usr/bin/env -Snode --no-warnings\nconsole.log(42);\n",
            "@node --no-warnings \"%~dp0\\..\\pkg\\cli.js\" %*\r\n",
            "exec node --no-warnings \"$basedir/../pkg/cli.js\" \"$@\"\n"),

        // abs-node
        ("../pkg/cli.js", "#!/usr/bin/node\nconsole.log(42);\n",
            "@node \"%~dp0\\..\\pkg\\cli.js\" %*\r\n",
            "exec node \"$basedir/../pkg/cli.js\" \"$@\"\n"),

        // env-bash
        ("../pkg/cli", "#!/usr/bin/env bash\necho hi\n",
            "@bash \"%~dp0\\..\\pkg\\cli\" %*\r\n",
            "exec bash \"$basedir/../pkg/cli\" \"$@\"\n"),

        // env-python
        ("../pkg/cli.py", "#!/usr/bin/env python3 -u\nprint(1)\n",
            "@python3 -u \"%~dp0\\..\\pkg\\cli.py\" %*\r\n",
            "exec python3 -u \"$basedir/../pkg/cli.py\" \"$@\"\n"),

        // bin-sh
        ("../pkg/cli.sh", "#!/bin/sh\necho hi\n",
            "@sh \"%~dp0\\..\\pkg\\cli.sh\" %*\r\n",
            "exec sh \"$basedir/../pkg/cli.sh\" \"$@\"\n"),

        // no-shebang-js
        ("../pkg/cli.js", "console.log(42);\n",
            "@node \"%~dp0\\..\\pkg\\cli.js\" %*\r\n",
            "exec node \"$basedir/../pkg/cli.js\" \"$@\"\n"),

        // no-shebang-mjs
        ("../pkg/cli.MJS", "console.log(42);\n",
            "@node \"%~dp0\\..\\pkg\\cli.MJS\" %*\r\n",
            "exec node \"$basedir/../pkg/cli.MJS\" \"$@\"\n"),

        // empty-cjs
        ("../pkg/cli.cjs", "",
            "@node \"%~dp0\\..\\pkg\\cli.cjs\" %*\r\n",
            "exec node \"$basedir/../pkg/cli.cjs\" \"$@\"\n"),

        // no-shebang-cmd
        ("../pkg/cli.cmd", "@echo hi\r\n",
            "@cmd /C \"%~dp0\\..\\pkg\\cli.cmd\" %*\r\n",
            "exec cmd /C \"$basedir/../pkg/cli.cmd\" \"$@\"\n"),

        // no-shebang-bat
        ("../pkg/cli.bat", "@echo hi\r\n",
            "@cmd /C \"%~dp0\\..\\pkg\\cli.bat\" %*\r\n",
            "exec cmd /C \"$basedir/../pkg/cli.bat\" \"$@\"\n"),

        // no-shebang-ps1
        ("../pkg/cli.ps1", "Write-Output hi\n",
            "@pwsh \"%~dp0\\..\\pkg\\cli.ps1\" %*\r\n",
            "exec pwsh \"$basedir/../pkg/cli.ps1\" \"$@\"\n"),

        // no-shebang-sh
        ("../pkg/cli.sh", "echo hi\n",
            "@sh \"%~dp0\\..\\pkg\\cli.sh\" %*\r\n",
            "exec sh \"$basedir/../pkg/cli.sh\" \"$@\"\n"),

        // no-shebang-noext
        ("../pkg/cli", "console.log(42);\n",
            "@\"%~dp0\\..\\pkg\\cli\" %*\r\n",
            "exec \"$basedir/../pkg/cli\" \"$@\"\n"),

        // no-shebang-exe
        ("../pkg/cli.exe", "MZ",
            "@\"%~dp0\\..\\pkg\\cli.exe\" %*\r\n",
            "exec \"$basedir/../pkg/cli.exe\" \"$@\"\n"),

        // scoped
        ("../@scope/pkg/bin/cli.js", "#!/usr/bin/env node\n",
            "@node \"%~dp0\\..\\@scope\\pkg\\bin\\cli.js\" %*\r\n",
            "exec node \"$basedir/../@scope/pkg/bin/cli.js\" \"$@\"\n"),

        // workspace
        ("../../my-bin.js", "#!/usr/bin/env node\n",
            "@node \"%~dp0\\..\\..\\my-bin.js\" %*\r\n",
            "exec node \"$basedir/../../my-bin.js\" \"$@\"\n"),

        // Not derived from cmd-shim, which pastes the arguments verbatim
        ("../pkg/cli.js", "#!/usr/bin/env -S node --title=50% --require ./it's.js\n",
            "@node ^\"--title=50%%^\" --require ^\"./it's.js^\" \"%~dp0\\..\\pkg\\cli.js\" %*\r\n",
            "exec node '--title=50%' --require './it'\"'\"'s.js' \"$basedir/../pkg/cli.js\" \"$@\"\n"),
    ];

    #[test]
    fn generates_relative_shims() {
        for (target_path, content, expected_cmd, expected_sh) in RELATIVE_SHIM_CASES {
            let target_path
                = Path::try_from(*target_path).unwrap();

            let target
                = RelativeShimTarget::new(&target_path, parse_shebang(content.as_bytes()));

            assert_eq!(make_relative_cmd_shim(&target_path, &target), format!("{}{}", RELATIVE_CMD_SHIM_PREFIX, expected_cmd), "{:?}", content);
            assert_eq!(make_relative_sh_shim(&target_path, &target), format!("{}{}", RELATIVE_SH_SHIM_PREFIX, expected_sh), "{:?}", content);
        }
    }
}
