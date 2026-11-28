use lvpa_rustc_wrapper::rules;
use lvpa_rustc_wrapper::sources;

use std::process::exit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut covered_crates = false;
    let mut covered_rules = false;
    let mut seams = false;
    let mut trees = false;

    for arg in &args {
        match arg.as_str() {
            "--covered-crates" => covered_crates = true,
            "--covered-rules" => covered_rules = true,
            "--seams" => seams = true,
            "--source-trees" => trees = true,
            other => {
                eprintln!(
                    "[lvpa-capability-panorama] FATAL: unknown selector `{other}`. Selectors: \
                     --covered-crates, --covered-rules, --seams, --source-trees."
                );
                exit(2);
            }
        }
    }

    if !(covered_crates || covered_rules || seams || trees) {
        covered_crates = true;
        covered_rules = true;
        seams = true;
    }

    if covered_crates {
        for package in rules::covered_crate_packages() {
            println!("{package}");
        }
    }
    if covered_rules {
        for (package, rule, unit) in rules::covered_crate_rules() {
            println!("{package}\t{rule}\t{unit}");
        }
    }
    if seams {
        let root = sources::workspace_root();
        for (module, host) in rules::seam_sources() {
            let landed = match &root {
                Some(root) => {
                    let path = root.join(host);
                    if path.is_file() {
                        "present"
                    } else {
                        "absent"
                    }
                }
                None => "unresolved",
            };
            println!("{module}\t{host}\t{landed}");
        }
    }
    if trees {
        match sources::trees() {
            Ok((repository, declared)) => {
                for tree in declared {
                    let root = sources::resolve_root(&tree, &repository);
                    let state = if root.is_dir() { "present" } else { "absent" };
                    println!("{}\t{}\t{state}", tree.id, sources::normalize(&root));
                }
            }
            Err(error) => {
                eprintln!("[lvpa-capability-panorama] FATAL: {error}");
                exit(3);
            }
        }
    }
}
