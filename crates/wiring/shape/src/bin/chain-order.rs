use std::env;
use std::process::exit;

use lvpa_wiring_shape::chain::{order_for, Entrance, ORDER};

fn main() {
    let mut names: Vec<String> = env::args().skip(1).collect();
    if names.is_empty() || names.len() == 1 && names[0] == "--all" {
        for node in ORDER.iter() {
            println!("{}", node.step.name());
        }
        return;
    }
    let guarantee = match names.remove(0).as_str() {
        "--guarantees" => true,
        other => {
            names.insert(0, other.to_string());
            false
        }
    };
    if names.len() != 1 {
        eprintln!("[lvpa-chain-order] pick one entrance: dev, build");
        exit(USAGE);
    }
    let entrance = match names[0].as_str() {
        "dev" => Entrance::Dev,
        "build" => Entrance::Build,
        other => {
            eprintln!("[lvpa-chain-order] `{other}` is not an entrance: dev, build");
            exit(USAGE);
        }
    };
    for step in order_for(entrance) {
        if guarantee {
            let held = step.guarantee();
            println!("{}\t{}\t{}", step.name(), held.on_disk.join(","), held.leaves.join(","));
        } else {
            println!("{}", step.name());
        }
    }
}

const USAGE: i32 = 2;
