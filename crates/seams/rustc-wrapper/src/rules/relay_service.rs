use std::path::Path;
use std::process::exit;

use super::{
    CrateSpec, Landing, Locate, Managed, Rule, Seam, SeamBody, SeamDependency, SeamHost, SeamSeed,
};
use crate::rewrite::{replace_an_edge, Footprint};

pub const SPEC: CrateSpec = CrateSpec {
    crate_path: "src/crates/services/relay-service/src",
    source_tree: "openbitfun",
    package: "openbitfun-relay-service",
    crate_name: "openbitfun_relay_service",
    package_module: None,
    root: None,
    dependencies: SPEC_DEPENDENCIES,
    seams: SEAMS,
    rules: SERVICE_RULES,
    carries: &[],
};

/// The edges the seams below state. The series the graph holds are stated by the tree's card
/// and by the table the pass lands them from, and no line is written above that card here.
const SPEC_DEPENDENCIES: &[SeamDependency] = &[];

/// Every card of the trees this repository takes, each at the path its tree holds it: a card
/// states its own edges, and the one table this repository keeps of series it lands reaches
/// only the card it names.
fn cards_of_the_trees() -> Vec<(String, std::path::PathBuf)> {
    let Ok((repository, trees)) = crate::sources::trees() else {
        eprintln!(
            "[lvpa-wrapper] FATAL: {} did not resolve, so no card of a taken tree can be found and no series it lands stands \
             anywhere. Nothing is rewritten.",
            crate::sources::DECLARATION_RELATIVE
        );
        exit(97);
    };
    let mut out: Vec<(String, std::path::PathBuf)> = Vec::new();
    for tree in &trees {
        let root = crate::sources::resolve_root(tree, &repository);
        let Ok(text) = std::fs::read_to_string(root.join("Cargo.toml")) else {
            continue;
        };
        for member in crate::sources::members_of(&text) {
            let card = root.join(&member).join("Cargo.toml");
            let Ok(body) = std::fs::read_to_string(&card) else {
                continue;
            };
            let Some(name) = package_of(&body) else {
                continue;
            };
            if !out.iter().any(|(held, _)| *held == name) {
                out.push((name, card));
            }
        }
    }
    out
}

/// The cards a landing pass may write into: the members the trees name, found the same way the
/// landing finds them, so the bytes kept beside the pass are the bytes of the cards it touched.
pub fn cards_to_keep() -> Vec<std::path::PathBuf> {
    cards_of_the_trees().into_iter().map(|(_, card)| card).collect()
}

fn package_of(card: &str) -> Option<String> {
    let mut inside = false;
    for line in card.lines() {
        let trimmed = line.trim_start_matches('\u{feff}').trim();
        if trimmed.starts_with('[') {
            inside = trimmed == "[package]";
            continue;
        }
        if !inside {
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("name") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        return Some(rest.trim().trim_matches(|one| one == '"' || one == '\'').to_string());
    }
    None
}

const SEAMS: &[Seam] = &[Seam {
    module: "lvpa_relay_sql_safety",
    host: SeamHost::checked("crates/seams/relay-sql-safety/src/lib.rs"),
    body: SeamBody::checked("crates/seams/relay-sql-safety/src/lib.rs"),
    seed: SeamSeed::Module,
    seeds: &["lvpa_relay_sql_safety"],
    dependencies: &[],
}];

/// The rules rewrite the statements the crate compiles and hold back from a test block that is not
/// in it: the crate is built with `--test` as well as as a library, a unit test writes its own
/// statements, and a statement this repository reads as one to hold back from must not reach a
/// test's graph. The block is the one the file's own `cfg(test)` opens.
const SERVICE_RULES: &[Rule] = &[
    Rule {
        id: "lvpa.relay_service.stated_as",
        file: "db.rs",
        hold_back_in: Some("cfg(test)"),
        locate: Locate::Surround(super::rename::Surround {
            from: "sqlx::query_as(",
            to: "crate::lvpa_relay_sql_safety::query_as(",
            seam: "lvpa_relay_sql_safety",
            assembled: None,
            row: None,
        }),
    },
    Rule {
        id: "lvpa.relay_service.stated",
        file: "db.rs",
        hold_back_in: Some("cfg(test)"),
        locate: Locate::Surround(super::rename::Surround {
            from: "sqlx::query(",
            to: "crate::lvpa_relay_sql_safety::query(",
            seam: "lvpa_relay_sql_safety",
            assembled: None,
            row: None,
        }),
    },
    Rule {
        id: "lvpa.relay_service.stated_scalar",
        file: "db.rs",
        hold_back_in: Some("cfg(test)"),
        locate: Locate::Surround(super::rename::Surround {
            from: "sqlx::query_scalar(",
            to: "crate::lvpa_relay_sql_safety::query_scalar(",
            seam: "lvpa_relay_sql_safety",
            assembled: None,
            row: None,
        }),
    },
];

/// The landing of the series the graph holds by a native library. `crate::edges::declared`
/// answers only for the crates whose own card states `links`: a crate the linker does not name
/// stands at the series its tree wrote, whether one tree wrote it or two, and no line of this
/// repository stands over it. The card a landed edge is read out of is the member the trees name
/// for that package, so no tree's path is written down here. The series a card is landed at is the
/// series the one table states for it: a value written here beside the table's would be a second
/// answer to a question the table already answers, and a card landed at the series its tree wrote
/// is a card landed nowhere.
const LANDED_NAME: &str = "libsqlite3_sys";
const LANDED_PACKAGE: &str = "libsqlite3-sys";
const LANDED_PATH: &str = "";

fn landed_one(edge: &crate::edges::Written) -> bool {
    edge.package == LANDED_PACKAGE
}

pub fn land_every_card(footprint: &mut Footprint) -> Vec<String> {
    let edges = crate::edges::declared();
    if edges.is_empty() {
        return Vec::new();
    }
    let cards = cards_of_the_trees();
    let mut landed_here = Vec::new();
    for (name, card) in cards {
        let mut mine: Vec<crate::edges::Written> = edges.iter().filter(|edge| edge.crate_name == name).cloned().collect();
        mine.sort_by(|one, other| one.package.cmp(&other.package));
        mine.dedup_by(|one, other| one.package == other.package);
        if mine.is_empty() {
            continue;
        }
        if !Path::new(&card).is_file() {
            eprintln!(
                "[lvpa-wrapper] FATAL: {} states the package {name}, and its card stands nowhere: the series {} lands for it \
                 would stand over no line",
                card.display(),
                mine.iter().map(|edge| edge.package.as_str()).collect::<Vec<&str>>().join(", ")
            );
            footprint.restore();
            exit(93);
        }
        for edge in mine {
            if !landed_one(&edge) {
                continue;
            }
            let landed_features: &'static [String] = Box::leak(edge.features_at(Path::new(&card)).into_boxed_slice());
            let dependency = SeamDependency {
                name: LANDED_NAME,
                package: LANDED_PACKAGE,
                version: edge.version_str(),
                path: LANDED_PATH,
                declared: "",
                features: landed_features,
                stands_down: false,
                landing: Landing::MemberManifest,
                managed: Managed::Replaced,
            };
            match replace_an_edge(&card, &dependency, footprint) {
                Ok(()) => landed_here.push(format!("{} {} at {}", edge.package, edge.version, card.display())),
                Err(error) => {
                    eprintln!(
                        "[lvpa-wrapper] FATAL: the edge {} the crate {name} states cannot be landed in {}: {error}",
                        edge.package,
                        card.display()
                    );
                    footprint.restore();
                    exit(93);
                }
            }
        }
    }
    landed_here
}

