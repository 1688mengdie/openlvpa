use std::env;
use std::process::exit;

use lvpa_wiring_shape::chain::{order_for, Entrance};

fn main() {
    let mut names: Vec<String> = env::args().skip(1).collect();
    if names.is_empty() {
        eprint!("[lvpa-chain-order] pick one entrance: dev, build");
        exit(2);
    }
    let guarantee = match names.remove(0).as_str() {
        "--guarantees" => true,
        other => {
            names.insert(0, other.to_string());
            false
        }
    };
    if names.len() != 1 {
        eprint!("[lvpa-chain-order] pick one entrance: dev, build");
        exit(2);
    }
    let entrance = match names.remove(0).as_str() {
        "dev" => Entrance::Dev,
        "build" => Entrance::Build,
        other => {
            eprintln!("[lvpa-chain-order] `{other}` is not an entrance: dev, build");
            exit(2);
        }
    };
    for step in order_for(entrance) {
        if guarantee {
            let g = step.guarantee();
            println!("{}\t{}\t{}", step.name(), g.on_disk.join(","), g.leaves.join(","));
        } else {
            println!("{}", step.name());
        }
    }
}
