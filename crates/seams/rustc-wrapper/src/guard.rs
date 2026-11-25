use std::env;
use std::path::{Path, PathBuf};
use std::process::exit;

pub const SENTINEL: &str = "LVPA_WRAPPER_ACTIVE";

pub const SWITCH: &str = "LVPA_WRAPPER";

pub fn wrapper_disabled() -> bool {
    matches!(
        env::var(SWITCH).ok().as_deref(),
        Some("off") | Some("OFF") | Some("0") | Some("false")
    )
}

pub fn reject_reentry() {
    if env::var(SENTINEL).is_ok() {
        eprintln!("[lvpa-wrapper] FATAL: recursive invocation detected (reentered while active)");
        exit(97);
    }
}

const WRAPPER_STEM: &str = "lvpa-rustc-wrapper";

pub fn compiler_invocation(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--crate-name" || arg.starts_with("--crate-name="))
}

pub fn take_real_compiler(args: &mut Vec<String>) -> String {
    let self_exe = env::current_exe().unwrap_or_default();
    match peel(args, &self_exe) {
        Peel::Compiler(real) => real,
        Peel::NoArguments => {
            eprintln!("[lvpa-wrapper] FATAL: no arguments (expected argv[1] = real rustc path)");
            exit(98);
        }
        Peel::NoCompiler { wrapper } => {
            eprintln!("[lvpa-wrapper] FATAL: real compiler path resolves to the wrapper itself: {wrapper}");
            exit(99);
        }
    }
}

enum Peel {
    Compiler(String),
    NoArguments,
    NoCompiler { wrapper: String },
}

fn peel(args: &mut Vec<String>, self_exe: &Path) -> Peel {
    if args.is_empty() {
        return Peel::NoArguments;
    }

    let mut peeled = 0;
    while args.len() > 1 && is_wrapper_layer(&args[0], self_exe) {
        let layer = args.remove(0);
        peeled += 1;
        eprintln!("[lvpa-wrapper] peeled wrapper layer {peeled}: {layer}");
    }

    let real = args.remove(0);

    if is_wrapper_layer(&real, self_exe) {
        Peel::NoCompiler { wrapper: real }
    } else {
        Peel::Compiler(real)
    }
}

fn is_wrapper_layer(arg: &str, self_exe: &Path) -> bool {
    let path = PathBuf::from(arg);
    if paths_equal(&path, self_exe) {
        return true;
    }
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.eq_ignore_ascii_case(WRAPPER_STEM))
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    let norm = |path: &Path| {
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_ascii_lowercase()
    };
    norm(a) == norm(b)
}
