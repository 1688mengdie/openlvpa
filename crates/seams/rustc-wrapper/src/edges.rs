use std::path::Path;

use crate::rewrite::Footprint;
use crate::sources;

pub const TABLE: &str = "scripts/extended-dependencies.json";

#[derive(Clone)]
pub struct Written {
    pub crate_name: String,
    pub name: String,
    pub path: Option<String>,
    pub package: String,
    pub version: String,
    pub features: Vec<String>,
    pub scope: Option<String>,
}


/// The edges this repository lands, read out of the one table that declares them. Each
/// entry names the crate whose card states the edge, the key the card writes it under,
/// and the series it is landed at; the file carries nothing else. An entry is a landing
/// only where the crate it reaches is one this repository takes: a row naming a crate no tree
/// states lands nothing, and a series read out of the `links` clause of whichever crate of the
/// registry happens to carry that name would be a value this table never wrote.
pub fn declared() -> Vec<Written> {
    let root = sources::repository_root();
    let path = root.join(TABLE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        eprintln!(
            "[lvpa-wrapper] FATAL: {} is not readable, so the edges this pass lands stand nowhere and cargo would \
             resolve the graphs the trees wrote instead. Nothing is rewritten.",
            path.display()
        );
        std::process::exit(97);
    };
    let table: serde_json::Value = match serde_json::from_str(&text) {
        Ok(parsed) => parsed,
        Err(error) => {
            eprintln!("[lvpa-wrapper] FATAL: {} does not read as the edge table ({error}), so no edge stands anywhere", path.display());
            std::process::exit(97);
        }
    };
    let mut out = Vec::new();
    let Some(lists) = table.as_object() else {
        eprintln!("[lvpa-wrapper] FATAL: {} holds no object, so it names no crate and no edge", path.display());
        std::process::exit(97);
    };
    for (at, list) in lists {
        if at.starts_with('_') {
            continue;
        }
        let Some(edges) = list.as_array() else {
            eprintln!("[lvpa-wrapper] FATAL: {} key `{at}` is not a list of edges", path.display());
            std::process::exit(97);
        };
        for edge in edges {
            let field = |key: &str| edge.get(key).and_then(|one| one.as_str()).map(str::to_string);
            let (Some(crate_name), Some(package)) = (field("crate"), field("package")) else {
                // an edge the table names without a version is reached by the path the table names
                // and not by a series: the card writes it, the landing does not touch it
                continue;
            };
            let features = edge
                .get("features")
                .and_then(|one| one.as_array())
                .map(|names| names.iter().filter_map(|one| one.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            out.push(Written {
                crate_name,
                name: field("name").unwrap_or_else(|| package.clone()),
                path: field("path"),
                package,
                version: field("version").unwrap_or_default(),
                features,
                scope: field("scope"),
            });
        }
    }
    out
}

/// The features the card states on the line that writes one package, read where the line stands:
/// the line is found by the key the card writes it under, and a card that renames the package
/// writes it under another key, so the package it names is read too. A card that states no line
/// for the package states no feature of it here.
pub fn written_features(card: &Path, name: &str, package: &str) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(card) else {
        return Vec::new();
    };
    for line in text.lines() {
        let trimmed = line.trim_start_matches('\u{feff}').trim();
        if trimmed.starts_with(['#', '[']) || !trimmed.contains('=') {
            continue;
        }
        let Some(key) = edge_key_of(trimmed) else {
            continue;
        };
        if key != name && edge_package_of(trimmed, key, false) != package {
            continue;
        }
        let features = features_of(trimmed);
        if !features.is_empty() {
            return features;
        }
    }
    Vec::new()
}

/// The series this table states for one edge, as the table wrote it: the card is landed at the one
/// series the table names, so the value handed to the landing is the value the table holds rather
/// than one this crate writes down beside it.
impl Written {
    pub fn version_str(&self) -> Option<&'static str> {
        let held: &'static str = Box::leak(self.version.clone().into_boxed_str());
        if held.is_empty() {
            None
        } else {
            Some(held)
        }
    }

    /// The features the landed line carries: the table's where it states them, and the card's own
    /// where it states none. A line landed without the features its tree wrote selects another
    /// crate of the same name - `libsqlite3-sys` without `bundled` carries no sqlite of its own,
    /// and the `bundled` of the driver above it then stands for nothing.
    pub fn features_at(&self, card: &Path) -> Vec<String> {
        if !self.features.is_empty() {
            return self.features.clone();
        }
        written_features(card, &self.name, &self.package)
    }
}

/// The edges a crate states in its own card are landed by the card's own landing, and no line is
/// written above a card here: the table holds the crates the graph admits once by a native
/// library, and a series it lands reaches the card the trees name for that package.
pub fn settle_cards() {}
/// The key a dependency line answers to: the name it is written under, or the package it
/// renames - a card that states `libsqlite3-sys = { package = "libsqlite3-sys", ... }` writes the
/// key the graph reads under the package, and the two spellings are one key.
pub fn edge_key_of(line: &str) -> Option<&str> {
    let body = line.splitn(2, '=').nth(1)?;
    if let Some(named) = body.split("package").nth(1).and_then(|rest| {
        let open = rest.find('"')?;
        let close = rest[open + 1..].find('"')?;
        Some(&rest[open + 1..open + 1 + close])
    }) {
        return Some(named);
    }
    let key = line.split('=').next()?.trim();
    let key = key.split('.').next()?.trim();
    if key.is_empty() || !key.chars().all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-') {
        return None;
    }
    Some(key)
}

/// The package a card's line states: a line that renames the crate names the package it lands, and
/// a line that writes a bare series names the crate by its key. A scoped table is the one a dotted
/// table in the workspace card opens, and a bare series there is the crate its key names.
pub fn edge_package_of(line: &str, key: &str, in_scope: bool) -> String {
    let Some(cut) = line.find('=') else {
        return key.to_string();
    };
    let rest = &line[cut + 1..];
    let body = rest.trim();
    let body = body.strip_prefix('{').unwrap_or(body);
    let body = body.rsplit_once('}').map(|(head, _)| head).unwrap_or(body);
    // the table a line states is read field by field, and a field is the key name `package`: a
    // value that spells the word is a value, not the key the package is stated under
    for part in split_fields(body) {
        let part = part.trim();
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        if name.trim() != "package" {
            continue;
        }
        let value = value.trim();
        let Some(quote @ ('"' | '\'')) = value.chars().next() else {
            continue;
        };
        return value[1..].split(quote).next().unwrap_or(key).to_string();
    }
    let _ = in_scope;
    key.to_string()
}

pub fn features_of(line: &str) -> Vec<String> {
    let Some(at) = line.find("features") else {
        return Vec::new();
    };
    let rest = &line[at + "features".len()..];
    let Some(open) = rest.find('[') else {
        return Vec::new();
    };
    let after = &rest[open + 1..];
    let Some(close) = after.find(']') else {
        return Vec::new();
    };
    after[..close]
        .split('"')
        .enumerate()
        .filter(|(index, _)| index % 2 == 1)
        .map(|(_, name)| name.to_string())
        .collect()
}

pub fn rest_of(line: &str) -> Vec<(String, String)> {
    let Some(cut) = line.find('=') else {
        return Vec::new();
    };
    let whole = line[cut + 1..].trim();
    let Some(body) = whole.strip_prefix('{').and_then(|rest| rest.rsplit_once('}').map(|(head, _)| head)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for part in split_fields(body) {
        let part = part.trim();
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let key = key.trim();
        // every key the landed line states is stated by the landing itself: a version or a feature
        // set carried from the card's own word would state the series the tree wrote beside the one
        // this table lands, and a package key carried beside the renamed key would name the package
        // twice; a workspace key beside a landed series asks for the series the parent card states,
        // which is the one line this landing replaces
        if key.is_empty() || key == "version" || key == "features" || key == "package" || key == "workspace" {
            continue;
        }
        out.push((key.to_string(), value.trim().to_string()));
    }
    out
}

/// The fields of an inline table, split where one ends: a value that is a list states its own
/// commas, and a split inside that list would read a feature name as a field.
fn split_fields(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut field = String::new();
    for one in body.chars() {
        match quote {
            Some(held) => {
                if one == held {
                    quote = None;
                }
                field.push(one);
            }
            None => match one {
                '"' | '\'' => {
                    quote = Some(one);
                    field.push(one);
                }
                '[' | '{' | '(' => {
                    depth += 1;
                    field.push(one);
                }
                ']' | '}' | ')' => {
                    depth = depth.saturating_sub(1);
                    field.push(one);
                }
                ',' if depth == 0 => {
                    out.push(std::mem::take(&mut field));
                }
                _ => field.push(one),
            },
        }
    }
    if !field.trim().is_empty() {
        out.push(field);
    }
    out
}

pub fn single_line(key: &str, version: &str, carried: &[(String, String)], features: &[String]) -> String {
    let mut fields: Vec<String> = Vec::new();
    if !version.is_empty() {
        fields.push(format!("version = \"{version}\""));
    }
    if !features.is_empty() {
        let names: Vec<String> = features.iter().map(|name| format!("\"{name}\"")).collect();
        fields.push(format!("features = [{}]", names.join(", ")));
    }
    // optional is the tree's own statement about when the crate is built, and a landing that
    // drops it builds the crate into every graph the card is read in
    for (name, value) in carried {
        fields.push(format!("{name} = {value}"));
    }
    format!("{key} = {{ {} }}", fields.join(", "))
}
/// One pass's landing: every card the table names is rewritten at the series the table names, and
/// what the trees wrote is kept beside the cards until the pass that closes the reading puts it
/// back. A graph cargo reads off these cards resolves the one instance of every `links` name; a
/// graph it reads off the cards as the trees wrote them resolves two and stops. The bytes kept are
/// the trees' own, so the card that stands between the two passes goes back to the tree it was
/// taken from and not to a row this repository would have had to write down.
pub fn keep() {
    let mut footprint = Footprint::default();
    keep_the_trees_own("take");
    land_every_card(&mut footprint);
    keep_the_trees_own("keep");
    footprint.forget();
}

/// The bytes a card is kept as: the trees' own, read and written without a lossy step, so the pass
/// that puts a card back writes the file the tree holds and not the text a reader would have made
/// of it. A file that does not decode is kept by the bytes it holds beside the card's path.
/// The bytes a card is kept as: the trees' own, read and written without a lossy step, so the pass
/// that puts a card back writes the file the tree holds and not the text a reader would have made
/// of it. A file whose bytes are not the text they decode to is kept by those bytes beside the path.
fn bytes_of(bytes: &[u8]) -> String {
    let held = String::from_utf8_lossy(bytes).into_owned();
    if held.as_bytes() == bytes {
        return format!("text:{held}");
    }
    let mut hex = String::from("bytes:");
    for one in bytes {
        hex.push_str(&format!("{one:02x}"));
    }
    hex
}

fn bytes_again(held: &str) -> Vec<u8> {
    if let Some(text) = held.strip_prefix("text:") {
        return text.as_bytes().to_vec();
    }
    let hex = held.strip_prefix("bytes:").unwrap_or_default();
    let mut out = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks(2) {
        if let Ok(one) = u8::from_str_radix(std::str::from_utf8(pair).unwrap_or("0"), 16) {
            out.push(one);
        }
    }
    out
}

fn keep_the_trees_own(which: &str) {
    let root = sources::repository_root();
    let held = root.join(TABLE).with_extension(format!("{which}.json"));
    let mut kept = serde_json::Map::new();
    for card in crate::rules::relay_service::cards_to_keep() {
        let at = card.to_string_lossy().to_string();
        if let Ok(bytes) = std::fs::read(&card) {
            kept.insert(at, serde_json::Value::String(bytes_of(&bytes)));
        }
    }
    let body = serde_json::to_string(&kept).unwrap_or_else(|error| {
        eprintln!(
            "[lvpa-wrapper] FATAL: the cards kept before the graph is read do not write as {} ({error}), so the trees \
             stand at the series this repository lands with nothing saying which row to put back",
            held.display()
        );
        std::process::exit(97);
    });
    if let Err(error) = std::fs::write(&held, body) {
        eprintln!(
            "[lvpa-wrapper] FATAL: {} cannot be written ({error}), so the bytes the trees wrote stand nowhere and the \
             pass that closes the reading puts no card back",
            held.display()
        );
        std::process::exit(97);
    }
}

pub fn put_back() {
    let root = sources::repository_root();
    let held = root.join(TABLE).with_extension("keep.json");
    let Ok(text) = std::fs::read_to_string(&held) else {
        return;
    };
    let Ok(kept) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text) else {
        eprintln!(
            "[lvpa-wrapper] FATAL: {} does not read as the cards kept before the graph was read, so the trees stay at \
             the series this repository landed and nothing here says which row to put back",
            held.display()
        );
        std::process::exit(97);
    };
    for (at, body) in kept {
        let Some(body) = body.as_str() else {
            continue;
        };
        let _ = std::fs::write(&at, bytes_again(body));
    }
    let _ = std::fs::remove_file(&held);
    let _ = std::fs::remove_file(root.join(TABLE).with_extension("take.json"));
}

/// Every card the table names, each landed at its own crate's edges and every other crate's
/// left as the tree wrote it: the edges are keyed by the crate that states them, so a card
/// answered for another crate would carry a series that crate never wrote. The card a series is
/// landed in is the one the trees name for that package, read where the package declares itself.
/// A pass started by hand names no root, so the cards are found where the root this process was
/// handed stands, and the root it takes for its own is put back before it returns.
pub fn land_every_card(footprint: &mut Footprint) -> Vec<String> {
    let root = sources::repository_root();
    let _taking = stated::Held::take(&root);
    crate::rules::relay_service::land_every_card(footprint)
}

/// The root a pass takes for its own while it stands: the file beside the binary answers only a run
/// that names no root, and a pass started by hand names none. The root the process was handed is
/// stated while the pass runs, and the word the binary carried stands again once it returns, so a
/// pass that names no root lands in the tree it was handed rather than in whatever tree a run above
/// last wrote down.
mod stated {
    use std::path::{Path, PathBuf};

    use crate::sources;

    pub struct Held(Option<Vec<u8>>);

    fn beside() -> Option<PathBuf> {
        let at = std::env::current_exe().ok()?;
        let directory = at.parent()?.to_path_buf();
        Some(directory.join(sources::ROOT_BESIDE))
    }

    impl Held {
        pub fn take(root: &Path) -> Self {
            let Some(beside) = beside() else {
                return Self(None);
            };
            let found = std::fs::read(&beside).ok();
            sources::state_the_root_beside_the_binary(root);
            Self(found)
        }
    }

    impl Drop for Held {
        fn drop(&mut self) {
            let Some(beside) = beside() else {
                return;
            };
            match self.0.take() {
                Some(bytes) => {
                    let _ = std::fs::write(&beside, bytes);
                }
                None => {
                    let _ = std::fs::remove_file(&beside);
                }
            }
        }
    }
}