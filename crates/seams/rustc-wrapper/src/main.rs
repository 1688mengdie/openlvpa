mod guard;

use lvpa_rustc_wrapper::rewrite;

use std::env;
use std::process::{exit, Command, Stdio};

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();

    if !guard::compiler_invocation(&args) {
        let child = std::thread::spawn(move || {
            let args = std::env::args().skip(1).collect::<Vec<String>>();
            if args.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "no command to forward",
                ));
            }
            let status = Command::new(&args[0])
                .args(args.iter().skip(1))
                .env_remove(guard::SENTINEL)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status();
            status
        })
        .join();
        let code = match child {
            Ok(Ok(status)) => status.code().unwrap_or(1),
            Ok(Err(error)) => {
                eprintln!("[lvpa-wrapper] FATAL: could not start the command: {error}");
                96
            }
            Err(_) => {
                eprintln!("[lvpa-wrapper] FATAL: the forwarded command could not be joined");
                96
            }
        };
        exit(code);
    }

    if guard::wrapper_disabled() {
        let real = guard::take_real_compiler(&mut args);
        exit(run(&real, &mut args, true));
    }

    guard::reject_reentry();
    let real_compiler = guard::take_real_compiler(&mut args);

    exit(run(&real_compiler, &mut args, false));
}

fn run(real_compiler: &str, args: &mut Vec<String>, passthrough: bool) -> i32 {
    if passthrough {
        eprintln!("[lvpa-wrapper] bypassed by {}=off: this pass compiles upstream as it stands", guard::SWITCH);
        return spawn(real_compiler, args);
    }

    let report = rewrite::apply(args);

    for id in &report.rewrote {
        eprintln!("[lvpa-wrapper] seam applied: {id}");
    }
    for miss in &report.misses {
        eprintln!(
            "[lvpa-wrapper] seam MISS: {} in {} / {} — its address did not resolve against the \
             tree the rule is written for",
            miss.rule_id, miss.crate_path, miss.file
        );
    }
    for id in &report.abandoned {
        eprintln!("[lvpa-wrapper] seam ABANDONED (precondition unmet): {id}");
    }
    for id in &report.gated {
        eprintln!("[lvpa-wrapper] seam applied under a #[cfg] gate: {id}");
    }
    for gate in &report.gates {
        eprintln!(
            "[lvpa-wrapper] seam gate {}: the rule table's declarations did not resolve on this \
             pass ({}), so the covered crate compiled as upstream",
            gate.declaration, gate.detail
        );
    }

    let status = spawn(real_compiler, args);

    if env::var("LVPA_WRAPPER_KEEP").is_err() {
        rewrite::restore(&report);
    }

    status
}

fn spawn(real_compiler: &str, args: &[String]) -> i32 {
    match Command::new(real_compiler)
        .args(args.iter())
        .env(guard::SENTINEL, "1")
        .status()
    {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("[lvpa-wrapper] FATAL: could not execute {real_compiler}: {error}");
            96
        }
    }
}
