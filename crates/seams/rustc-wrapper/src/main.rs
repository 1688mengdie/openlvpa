mod guard;

use lvpa_rustc_wrapper::{edges, rewrite};

use std::env;
use std::process::{exit, Command, Stdio};

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();

    // the root is settled before any compiler starts, and every compiler this run starts -- this
    // one and the rustc underneath it -- is handed the same answer: the declaration a repository
    // states for itself, read where it stands, and the root stated beside this binary for a
    // compiler whose working directory belongs to a crate of the registry
    if let Some(root) = lvpa_rustc_wrapper::sources::workspace_root() {
        let declaration = root.join(lvpa_rustc_wrapper::sources::DECLARATION_RELATIVE);
        if declaration.is_file() {
            env::set_var(lvpa_rustc_wrapper::sources::DECLARATION_ENV, &declaration);
        }
        env::set_var(lvpa_rustc_wrapper::sources::REPOSITORY_ENV, &root);
        lvpa_rustc_wrapper::sources::state_the_root_beside_the_binary(&root);
    }

    if args.len() == 1 && args[0] == SETTLE {
        edges::keep();
        return;
    }

    // the graph is read before a crate is handed to the compiler, and cargo reads the manifest
    // entries and the crate roots before it asks for a source: the one line of a card that
    // stands in a window no other tree admits, and the module a seam lands, are settled at the
    // pass that sees the card and the crate before the sources are read
    let answered = args.first().is_some_and(|first| first == METADATA || first == MIRROR);
    if answered {
        rewrite::stand_down_before_the_graph();
        rewrite::lay_every_seam();
        return;
    }

    // the cards stand at the series the table lands them at while a graph is read off them: one
    // pass writes them, the pass that closes the reading puts each one back the way its tree wrote
    // it, so no byte of the landing stands in a tree between the two
    if args.len() == 1 && args[0] == LAND {
        edges::keep();
        return;
    }

    if args.len() == 1 && args[0] == PUT_BACK {
        edges::put_back();
        return;
    }


    if !guard::compiler_invocation(&args) {
        let root = root_now();
        let child = std::thread::spawn(move || {
            let args = std::env::args().skip(1).collect::<Vec<String>>();
            if args.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "no command to forward",
                ));
            }
            // the command this run forwards starts where this run stood: a crate of the registry
            // reads the declaration exactly as the pass above did
            let mut launched = Command::new(&args[0]);
            launched
                .args(args.iter().skip(1))
                .env_remove(guard::SENTINEL)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit());
            if let Some(root) = &root {
                let declaration = root.join(lvpa_rustc_wrapper::sources::DECLARATION_RELATIVE);
                if declaration.is_file() {
                    launched.env(lvpa_rustc_wrapper::sources::DECLARATION_ENV, declaration);
                }
                launched.env(lvpa_rustc_wrapper::sources::REPOSITORY_ENV, root);
                launched.env(lvpa_rustc_wrapper::sources::RESOLVED_ENV, "");
            }
            let status = launched.status();
            status
        })
        .join();
        let code = match child {
            Ok(Ok(status)) => status.code().unwrap_or(1),
            Ok(Err(error)) => {
                eprintln!("[lvpa-wrapper] FATAL: could not start the command {:?}: {error}", args[0]);
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
        return spawn(real_compiler, args);
    }

    let report = rewrite::apply(args);

    for id in &report.rewrote {
        eprintln!("[lvpa-wrapper] seam applied: {id}");
    }
    for miss in &report.misses {
        eprintln!(
            "[lvpa-wrapper] FATAL: the rule {} in {} / {} did not resolve against the tree it is written for, so \
             the covered crate would compile as upstream with its seam absent.",
            miss.rule_id, miss.crate_path, miss.file
        );
    }
    for id in &report.abandoned {
        eprintln!("[lvpa-wrapper] FATAL: the seam {id} was abandoned because a precondition was unmet.");
    }
    for gate in &report.gates {
        eprintln!(
            "[lvpa-wrapper] FATAL: seam gate {}: the rule table's declarations did not resolve on this \
             pass ({}), so the covered crate would compile as upstream with its seam absent.",
            gate.declaration, gate.detail
        );
    }

    if report.broke_its_seam() {
        eprintln!(
            "[lvpa-wrapper] FATAL: the seams this pass covers did not land as declared, so the assembly \
             would compile as upstream with them absent. Nothing reached the compiler and no build output \
             was written for this crate."
        );
        rewrite::restore(&report);
        return SEAM_BROKEN;
    }
    let status = spawn(real_compiler, args);

    rewrite::restore(&report);

    status
}
fn root_now() -> Option<std::path::PathBuf> {
    lvpa_rustc_wrapper::sources::workspace_root()
}

fn spawn(real_compiler: &str, args: &[String]) -> i32 {
    // the compiler this run was resolved for starts with the root settled: a crate of the registry
    // reads the declaration exactly as this one did, and a nested run that inherits the answer
    // still finds it where it was found here
    let mut command = Command::new(real_compiler);
    command.args(args.iter()).env(guard::SENTINEL, "1");
    if let Some(root) = root_now() {
        let declaration = root.join(lvpa_rustc_wrapper::sources::DECLARATION_RELATIVE);
        if declaration.is_file() {
            command.env(lvpa_rustc_wrapper::sources::DECLARATION_ENV, declaration);
        }
        command.env(lvpa_rustc_wrapper::sources::REPOSITORY_ENV, root);
        command.env(lvpa_rustc_wrapper::sources::RESOLVED_ENV, "");
    }
    match command.status() {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("[lvpa-wrapper] FATAL: could not execute {real_compiler}: {error}");
            96
        }
    }
}

const SEAM_BROKEN: i32 = 92;

/// The one argument this binary answers itself: the chain resolves its graph before any
/// crate of it is compiled, and the edges a tree's members state in their own cards are
/// landed before that reading rather than during it.
const SETTLE: &str = "--settle-member-edges";
const METADATA: &str = "--entry-table";
const MIRROR: &str = "--held-seams";

/// The chain's own step: every card that states an edge the table lands is rewritten before
/// the graph is read, and the reading is what the chain asks cargo for next. A wrapper a
/// compiler invocation carries is a compiler wrapper and sees no crate that needs no
/// compiling, so the step that resolves the graph is this binary's own reading of argv.
const LAND: &str = "--land-card-edges";
const PUT_BACK: &str = "--put-back-the-cards";
