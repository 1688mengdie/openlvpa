use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use crate::rules::{self, Locate, Rule};

pub struct Miss {
    pub rule_id: String,
    pub crate_path: &'static str,
    pub file: &'static str,
}

pub struct Restored {
    pub path: PathBuf,
    pub original: Option<Vec<u8>>,
}

#[derive(Default)]
pub struct Footprint {
    pub files: Vec<Restored>,
    pub dirs: Vec<PathBuf>,
}

impl Footprint {
    /// Puts back every file and removes every directory this pass created, newest first, so a
    /// pass that stops half way leaves the tree as it found it.
    pub fn restore(&mut self) {
        for entry in self.files.drain(..).rev() {
            match entry.original {
                Some(bytes) => {
                    let _ = fs::write(&entry.path, bytes);
                }
                None => {
                    let _ = fs::remove_file(&entry.path);
                }
            }
        }
        for dir in self.dirs.drain(..).rev() {
            let _ = fs::remove_dir(&dir);
        }
    }

    /// Lets go of what was recorded and leaves the files as the pass wrote them: a card landed
    /// before the graph is read is landed for the graph, which is read after the pass returns.
    pub fn forget(&mut self) {
        self.files.clear();
        self.dirs.clear();
    }
}

pub struct Gate {
    pub declaration: &'static str,
    pub detail: String,
}

pub struct Report {
    pub rewrote: Vec<String>,
    pub misses: Vec<Miss>,
    pub abandoned: Vec<String>,
    pub footprint: Footprint,
    pub record: Option<PathBuf>,
    pub gated: Vec<String>,
    pub gates: Vec<Gate>,
}

impl Report {
    pub fn any(&self) -> bool {
        !self.rewrote.is_empty()
    }

    pub fn broke_its_seam(&self) -> bool {
        !self.gates.is_empty() || !self.misses.is_empty() || !self.abandoned.is_empty()
    }

    pub fn self_check(&self, covered: usize, reached_upstream: usize) -> Option<String> {
        if !self.misses.is_empty() {
            let lines: Vec<String> = self
                .misses
                .iter()
                .map(|miss| {
                    format!(
                        "  - {} ({} / {}): its address did not resolve against the tree the rule is \
                         written for",
                        miss.rule_id, miss.crate_path, miss.file
                    )
                })
                .collect();
            return Some(format!(
                "{} rule(s) did not resolve against the tree they are written for, so the \
                 covered crate(s) would compile as upstream with the seam absent:\n{}",
                self.misses.len(),
                lines.join("\n")
            ));
        }
        if self.any() && reached_upstream != 0 && covered == reached_upstream {
            return Some(format!(
                "{} rule(s) reported as applied ({}), but a rewritten source reached the compiler \
                 for none of them - the build would silently keep upstream behaviour",
                self.rewrote.len(),
                self.rewrote.join(", ")
            ));
        }
        if !self.gates.is_empty() {
            let lines: Vec<String> = self
                .gates
                .iter()
                .map(|gate| format!("  - {}: {}", gate.declaration, gate.detail))
                .collect();
            return Some(format!(
                "{} declaration(s) the rule table is written against did not resolve on this pass, so the crate \
                 they cover would compile as upstream with its seam absent:\n{}",
                self.gates.len(),
                lines.join("\n")
            ));
        }
        if !self.abandoned.is_empty() && self.any() {
            return Some(format!(
                "{} rule(s) reported as applied while {} rule(s) were abandoned because a \
                 precondition was unmet - the crate would ship with a partially active seam. \
                 Abandoned: {}",
                self.rewrote.len(),
                self.abandoned.len(),
                self.abandoned.join(", ")
            ));
        }
        None
    }
}

/// One line of a card stands in a window no other tree admits, and the graph on the other side of
/// it is a conflict cargo reports before this pass sees a crate: the line is taken down before the
/// graph is read, at the pass that sees the cards first.
/// The line is found by what it states, not by where it sits: the key the card writes, the package
/// the entry table lands, and the series the card states for it. The card is the member a tree's own
/// manifest names for the package, so the line is read in the card of the crate that writes it.
pub fn stand_down_before_the_graph() {
    let mut stood: Vec<(&'static str, PathBuf, String)> = Vec::new();
    for spec in rules::registered() {
        let mut standing: Vec<&'static rules::SeamDependency> = spec
            .dependencies
            .iter()
            .filter(|dependency| dependency.stands_down)
            .collect();
        for seam in spec.seams {
            standing.extend(seam.dependencies.iter().filter(|dependency| dependency.stands_down));
        }
        let Some(card) = rules::card_of(spec) else {
            continue;
        };
        let Ok(manifest) = fs::read_to_string(&card) else {
            continue;
        };
        for dependency in standing {
            let Some(series) = dependency.version else {
                continue;
            };
            let marker = format!("version = \"{series}\"");
            for row in manifest.lines() {
                let row = row.trim();
                if row.starts_with('#') || row.starts_with('[') {
                    continue;
                }
                let Some((name, rest)) = row.split_once('=') else {
                    continue;
                };
                let name = name.trim();
                if name.is_empty() || name.contains(char::is_whitespace) {
                    continue;
                }
                if !rest.contains(&marker) {
                    continue;
                }
                if stood
                    .iter()
                    .any(|(_, at, taken): &(&'static str, PathBuf, String)| at == &card && taken == name)
                {
                    continue;
                }
                stood.push((dependency.package, card.clone(), name.to_string()));
            }
        }
    }
    for (package, at, key) in stood {
        let Ok(manifest) = fs::read_to_string(&at) else {
            continue;
        };
        // the taken line is the one the table filtered out here, not the line a landing writes: the
        // landing above states that same key with the series this table lands, and it stands as the
        // one statement of the key
        let statement = taken_line(&manifest, &package, &key);
        let kept: Vec<&str> = manifest
            .lines()
            .filter(|row| {
                !crate::edges::edge_key_of(row).is_some_and(|held| {
                    held == key && crate::edges::edge_package_of(row, held, false) == package
                })
            })
            .collect();
        let mut body = kept.join("\n");
        body.push('\n');
        if let Some(line) = statement {
            // the key stands once, on the series this table lands it at
            body.push_str(&format!("{line}\n"));
        }
        fs::write(&at, body).unwrap_or_else(|error| {
            eprintln!(
                "[lvpa-wrapper] FATAL: {} cannot be read down at `{key}`: {error}",
                at.display()
            );
            exit(93);
        });
    }
}
/// The one statement a taken key stands as: the key on the series this table lands it at, carrying
/// the features and the keys the tree wrote beside it. A card that already states the key on that
/// series already carries the landing, and the landing stands.
fn taken_line(manifest: &str, package: &str, key: &str) -> Option<String> {
    let landed: Vec<&'static str> = crate::rules::registered()
        .iter()
        .flat_map(|spec| spec.dependencies.iter())
        .filter(|dependency| dependency.package == package && !dependency.stands_down)
        .filter_map(|dependency| dependency.version)
        .collect();
    let own = crate::rules::registered()
        .iter()
        .flat_map(|spec| spec.dependencies.iter())
        .filter(|dependency| dependency.package == package && dependency.stands_down)
        .collect::<Vec<_>>();
    let mut features: Vec<String> = Vec::new();
    let mut carried: Vec<(String, String)> = Vec::new();
    let mut series = "";
    for row in manifest.lines() {
        if crate::edges::edge_key_of(row) != Some(key) || crate::edges::edge_package_of(row, key, false) != package {
            continue;
        }
        if landed.iter().any(|held| row.contains(&format!("version = \"{held}\""))) {
            return None;
        }
        for name in crate::edges::features_of(row) {
            if !features.contains(&name) {
                features.push(name);
            }
        }
        for (name, value) in crate::edges::rest_of(row) {
            if !carried.iter().any(|(held, _)| held == &name) {
                carried.push((name, value));
            }
        }
    }
    for dependency in &own {
        if let Some(held) = dependency.version {
            series = held;
        }
    }
    Some(crate::edges::single_line(key, series, &carried, &features))
}

pub fn apply(args: &mut Vec<String>) -> Report {
    let mut report = Report {
        rewrote: Vec::new(),
        misses: Vec::new(),
        abandoned: Vec::new(),
        footprint: Footprint::default(),
        record: None,
        gated: Vec::new(),
        gates: Vec::new(),
    };

    for record in outstanding_records() {
        match classify_settled(&record) {
            OwnerState::Alive => {}
            OwnerState::Unknown => {
                eprintln!(
                    "[lvpa-wrapper] FATAL: the record {} cannot be classified: its companion lock \
                     could not be read, so whether the run that wrote it is still compiling is \
                     undecidable and the tree may still carry its output.",
                    record.display()
                );
                exit(95);
            }
            OwnerState::Gone => {
                if let Err(error) = undo_recorded_run(&record) {
                    eprintln!(
                        "[lvpa-wrapper] FATAL: {} The covered tree cannot be read back to the state \
                         a previous run found, so nothing was rewritten against it.",
                        error
                    );
                    exit(95);
                }
            }
        }
    }

    if writes_a_covered_crate() {
        for (declaration, detail) in rules::unresolved_declarations() {
            eprintln!(
                "[lvpa-wrapper] FATAL: the rule table's declaration `{declaration}` did not resolve, so \
                 {detail}. Nothing is rewritten against an unresolved table."
            );
            exit(97);
        }
    }

    match rules::trees_state() {
        Ok(()) => {}
        Err(reason) => {
            report.gates.push(Gate {
                declaration: rules::DECLARATION,
                detail: reason,
            });
            return report;
        }
    }

    if let Err(error) = begin_run() {
        eprintln!(
            "[lvpa-wrapper] FATAL: this run cannot record what it is about to write ({error}). A \
             write it cannot take back is a change to upstream's own sources, so nothing is \
             rewritten."
        );
        finish_run();
        restore_then_exit(&report, 95);
    }

    match rules::declaration_state() {
        Ok(()) => {}
        Err(detail) => {
            eprintln!("[lvpa-wrapper] FATAL: {detail}");
            finish_run();
            restore_then_exit(&report, 95);
        }
    }

    let mut roots = Vec::new();
    for arg in args.iter() {
        if !arg.ends_with(".rs") || arg.starts_with('-') {
            continue;
        }
        roots.push(PathBuf::from(arg));
    }
    let Some(root) = roots.pop() else {
        return report;
    };
    let root = match fs::canonicalize(&root) {
        Ok(abs) => strip_unc(abs),
        Err(_) => root,
    };
    let Some(crate_src) = root.parent().map(Path::to_path_buf) else {
        return report;
    };

    let subject = crate_src.to_string_lossy().replace('\\', "/");
    // a statement this crate writes and the series this repository compiles it at reads otherwise
    // is carried before a rule reads the file: the source a rule sees is the source the compiler
    // gets, and a line that moved stops the pass rather than landing half a carry
    carry_every_statement(&mut report, crate_src.parent().map(Path::to_path_buf).unwrap_or_else(|| root.clone()).as_path(), &crate_src);
    let spec = match rules::cover(&subject) {
        rules::Coverage::Claimed(spec) => spec,
        rules::Coverage::Uncovered => return report,
        rules::Coverage::Absorbed => return report,
        rules::Coverage::Orphan(spec) => {
            eprintln!(
                "[lvpa-wrapper] FATAL: {subject} sits inside the tree declared as {}, yet no crate \
                 declaration claims it. A crate the declaration does not name would be built \
                 without the seam it owes, so the pass stops here.",
                spec.source_tree
            );
            exit(93);
        }
        rules::Coverage::Ruleless(spec) => {
            eprintln!(
                "[lvpa-wrapper] FATAL: {subject} is claimed by the spec for {}, and that spec \
                 carries no rule that lands in it. A spec with no rule covers nothing, so the \
                 pass stops here.",
                spec.package
            );
            exit(93);
        }
    };

    let covered = 1usize;
    let Some(repository) = workspace_root_path() else {
        eprintln!(
            "[lvpa-wrapper] FATAL: the workspace root could not be resolved, so covered crate {} cannot carry any \
             seam it declares and nothing tells a seam that landed from one that did not. Nothing is rewritten.",
            spec.crate_path
        );
        exit(97);
    };
    let mut missing_seams: Vec<(&str, String)> = Vec::new();
    for seam in spec.seams {
        let source = repository.join(seam.host.as_str_checked());
        if !source.is_dir() && !source.is_file() {
            let reason = format!("{} is not a readable file", source.display());
            missing_seams.push((seam.module, reason));
        }
    }
    if !missing_seams.is_empty() {
        for (module, reason) in &missing_seams {
            eprintln!(
                "[lvpa-wrapper] FATAL: covered crate {} cannot carry seam {module}: {reason}",
                spec.crate_path
            );
        }
        report.abandoned.extend(
            spec.rules
                .iter()
                .map(|rule| rule.reported_id(spec.source_tree)),
        );
        if let Some(reason) = report.self_check(covered, 0) {
            eprintln!("[lvpa-wrapper] FATAL: seam self-check failed: {reason}");
            restore_then_exit(&report, 93);
        }
        return report;
    }

    let Ok(root) = std::fs::canonicalize(&root) else {
        eprintln!(
            "[lvpa-wrapper] FATAL: the covered crate root {subject} declares {package} but the \
             compiler was handed no file of it, so the seam has nowhere to land",
            package = spec.package
        );
        restore_then_exit(&report, 93);
    };
    let _ = strip_unc(root);

    // the card a covered crate is built from states the series the graph is resolved at before the
    // compiler reads one byte of the crate: a seam states the same edges its crate states, and a
    // card left at the tree's own series would resolve a graph the seam does not stand in. The card
    // is the crate's own, found by the package it names and never by a path written here
    crate::edges::settle_cards();

    let injected = match inject(&crate_src, spec) {
        Ok(injected) => Some(injected),
        Err(error) => {
            eprintln!(
                "[lvpa-wrapper] FATAL: could not inject the seam for {}: {error}",
                crate_src.display()
            );
            None
        }
    };
    match injected {
        Some(injected) => {
            report
                .rewrote
                .extend(injected.rules.0.iter().map(|id| qualified_id(spec, id)));
            for id in injected.rules.1 {
                let miss = describe_miss(spec, &id);
                report.misses.push(miss);
            }
            report.abandoned.extend(injected.abandoned);
            for entry in injected.footprint.files {
                if report
                    .footprint
                    .files
                    .iter()
                    .any(|saved| saved.path == entry.path)
                {
                    continue;
                }
                report.footprint.files.push(entry);
            }
            report.footprint.dirs.extend(injected.footprint.dirs);
            LIVE_LOCKS.with(|cell| cell.borrow_mut().push(injected.lock));
        }
        None => {
            report.abandoned.extend(
                spec.rules
                    .iter()
                    .map(|rule| rule.reported_id(spec.source_tree)),
            );
        }
    }

    if let Some(reason) = report.self_check(covered, 0) {
        eprintln!("[lvpa-wrapper] FATAL: seam self-check failed: {reason}");
        restore_then_exit(&report, 93);
    }

    match record_run(&report.footprint) {
        Ok(record) => report.record = record,
        Err(error) => {
            eprintln!(
                "[lvpa-wrapper] FATAL: this pass wrote to the covered tree but could not record \
                 what it wrote ({error}). The tree is put back and the build stops."
            );
            restore_then_exit(&report, 93);
        }
    }

    report
}

fn qualified_id(spec: &'static rules::CrateSpec, id: &str) -> String {
    match spec.rules.iter().find(|rule| rule.id == id) {
        Some(rule) => rule.reported_id(spec.source_tree),
        None => format!("{}.{}", spec.source_tree, id),
    }
}

fn describe_miss(spec: &'static rules::CrateSpec, id: &str) -> Miss {
    if let Some(module) = id.strip_prefix("module_decl.") {
        return Miss {
            rule_id: format!("{}.{id}", spec.source_tree),
            crate_path: spec.crate_path,
            file: spec
                .seams
                .iter()
                .find(|seam| seam.module == module)
                .map(|seam| seam.host.as_str_checked())
                .unwrap_or("<unregistered>"),
        };
    }
    match spec.rules.iter().find(|rule| rule.id == id) {
        Some(rule) => Miss {
            rule_id: rule.reported_id(spec.source_tree),
            crate_path: spec.crate_path,
            file: rule.file,
        },
        None => Miss {
            rule_id: format!("{}.{id}", spec.source_tree),
            crate_path: spec.crate_path,
            file: "<unregistered>",
        },
    }
}

fn workspace_root_path() -> Option<PathBuf> {
    crate::sources::workspace_root()
}

struct InjectResult {
    rules: (Vec<String>, Vec<String>),
    abandoned: Vec<String>,
    footprint: Footprint,
    lock: InjectionLock,
}

fn inject(
    crate_src: &Path,
    spec: &'static rules::CrateSpec,
) -> std::io::Result<InjectResult> {
    let injection_lock = take_injection_lock(crate_src)?;
    let seams = spec.seams;
    let mut applied = Vec::new();
    let mut missed = Vec::new();
    let mut abandoned = Vec::new();
    let mut footprint = Footprint::default();
    let host_manifest = match crate_src
        .ancestors()
        .find(|dir| dir.join("Cargo.toml").is_file())
        .map(|dir| dir.join("Cargo.toml"))
    {
        Some(manifest) => manifest,
        None => unreachable_landing(
            crate_src,
            None,
            "no manifest stands above the crate, so no edge of its host can be replaced",
        ),
    };

    let mut declared_modules: Vec<(&'static str, &'static [&'static str])> = Vec::new();
    let mut seam_dependencies: Vec<&'static [rules::SeamDependency]> = Vec::new();
    // the seams are the ones this crate's rules name: a seam is declared once for the tree it
    // belongs to, and the tree is read for every crate it holds
    let mut wanted: Vec<&'static str> = spec.rules.iter().filter_map(rule_seam_module).collect();
    // every card this pass states an edge in is read once, here, and written once at the end: a
    // line landed while another edge's line is still being decided is a line the tree wrote that the
    // next decision reads as a landing
    let mut cards: BTreeMap<PathBuf, Vec<&'static rules::SeamDependency>> = BTreeMap::new();
    for dependency in spec.dependencies.iter().filter(|dependency| dependency.stands_down) {
        cards.entry(host_manifest.clone()).or_default().push(dependency);
    }
    // a seam whose body the crate reads is stated beside its sources; every seam edge is a line of
    // the card, and a seam the crate carries no file for states no line at all
    for dependency in spec
        .dependencies
        .iter()
        .filter(|dependency| matches!(dependency.landing, rules::Landing::SeamModule))
    {
        cards.entry(host_manifest.clone()).or_default().push(dependency);
    }
    let carried: Vec<&'static rules::Seam> = seams
        .iter()
        .filter(|seam| wanted.contains(&seam.module))
        .collect();
    for seam in carried {
        let Some(host) = workspace_root_path().map(|root| root.join(seam.host.as_str_checked()))
        else {
            missed.push(format!("module_decl.{}", seam.module));
            continue;
        };
        // a copy that already stands is written already: writing it again over its own seed
        // records a file the pass made as a file the tree held, and the restore then puts the
        // copy back for good
        // a seam of the crate this pass covers is landed in its own sources, but the edges its body
        // is read through are stated here whether or not the copy stands: a second command reads the
        // copy the first one wrote, and the body it holds reads the crate by the name it is bound at
        let seam_seed = seam.seeds.first().copied().unwrap_or("lib.rs");
        let flat = seam_seed.ends_with(".rs");
        if flat && crate_src.join(seam_seed).is_file() {
            wanted.retain(|one| *one != seam.module);
            declared_modules.push((seam.module, seam.seeds));
            continue;
        }
        if !flat && crate_src.join(seam.module).join("mod.rs").is_file() {
            let _ = declare_module(&crate_src.of_the_crate(), seam.module, &mut footprint);
            declared_modules.push((seam.module, seam.seeds));
            seam_dependencies.push(seam.dependencies);
            continue;
        }
        // a seam that did not land states no edge: its card entry would carry a caller whose
        // module the crate does not hold
        match write_seam_module(&host, crate_src, seam, &mut footprint, spec.rules) {
            Ok(Some(declaration)) => {
                declared_modules.push(declaration);
                seam_dependencies.push(seam.dependencies);
            }
            Ok(None) => {
            }
            Err(error) => {
                abandoned.push(format!("module_decl.{}: {error}", seam.module))
            }
        }
    }
    // the covered crate states the module in its own root file: the copy is read by name from that
    // crate, so the line and the copy stand in one crate and not two
    for (module, _seeds) in &declared_modules {
        let statement_root = crate_src.of_the_crate();
        if !statement_root.is_file() {
            continue;
        }
        match declare_module(&statement_root, module, &mut footprint) {
            Ok(()) => {}
            Err(error) => abandoned.push(format!("module_decl.{module}: {error}")),
        }
    }

    for dependency in spec
        .dependencies
        .iter()
        .filter(|dependency| matches!(dependency.landing, rules::Landing::SeamModule))
    {
        // a card is read once per run, before the sources are read: an edge written after the
        // resolver has run is an edge the crate this pass rewrites is compiled without
        if dependency.managed.adds_an_edge() {
            let body = match workspace_root_path().map(|root| root.join(dependency.path).join("Cargo.toml")) {
                Some(manifest) if manifest.is_file() => manifest,
                _ => {
                    missed.push(format!("dependency.{}.Cargo.toml", dependency.package));
                    continue;
                }
            };
            let seam_card = workspace_root_path()
                .map(|root| root.join(dependency.path).join("Cargo.toml"))
                .filter(|card| card.is_file());
            // a crate states no edge to itself, so a seam crate that names its own package in the
            // seam's dependency table carries that edge already and needs no second statement; the
            // card the host names as the landing's directory is the same card when the landing
            // lands the edge in the card the host names, and it states the package once for both
            if body == host_manifest || Some(body.as_path()) == seam_card.as_deref() {
                continue;
            }
            cards.entry(body).or_default().push(dependency);
        }
    }
    // the edges of a covered crate stand before the sources are read, and the card states every one
    // of them on one pass: the line is written once, and the next decision reads it as a card
    let mut cards: Vec<(PathBuf, Vec<&'static rules::SeamDependency>)> = cards.into_iter().collect();
    for (_, dependencies) in &mut cards {
        dependencies.sort_by(|one, other| one.package.cmp(other.package));
        dependencies.dedup_by(|one, other| one.package == other.package);
    }
    for (card, dependencies) in cards {
        let mut text = normalize(&fs::read_to_string(&card)?);
        for dependency in dependencies {
            text = match state_the_edge(&text, dependency, &card, crate_src) {
                Ok(text) => text,
                Err(error) => unreachable_landing(
                    crate_src,
                    None,
                    &format!("dependency.{}.{}: {error}", dependency.package, card.display()),
                ),
            };
        }
        write_in_place(&card, text.into_bytes(), &mut footprint)?;
    }
    let mut extern_crates: Vec<String> = Vec::new();
    let mut named: Vec<String> = Vec::new();
    for dependency in seam_dependencies.iter().copied().flatten() {
        if let Some(name) = dependency.extern_crate() {
            let bound = dependency.package.replace('-', "_");
            if !extern_crates.contains(&bound) {
                extern_crates.push(bound.clone());
            }
            if !named.contains(&name) {
                named.push(name);
            }
        }
    }
    let names = if named.is_empty() { String::new() } else { format!("use {};\n", named.join(", ")) };
    for path in seam_module_paths(crate_src, &declared_modules) {
        let mut module_text = normalize(&fs::read_to_string(&path)?);
        // a binding an earlier pass wrote is re-stated by this pass from the head of the body:
        // dropped from both places, it stands once, at the one place the body is read from
        if !extern_crates.is_empty() {
            module_text = module_text
                .lines()
                .filter(|line| {
                    !extern_crates.iter().any(|name| line.trim() == format!("extern crate {name};"))
                })
                .map(|line| format!("{line}\n"))
                .collect();
        }
        for source in seam_dependencies.iter().copied().flatten() {
            let declared = source.declared_name();
            if source.name != declared {
                module_text = module_text.replace(
                    &format!("use {}::", source.name.replace('-', "_")),
                    &format!("use {declared}::"),
                );
            }
        }
        // the name the seam lands under is read by the body as the seam writes it, and the binding
        // stands with the crate the body calls through: the body states the call, the head states
        // the name the call is read by
        let mut landed = declare_extern_crates(&module_text, &extern_crates);
        landed = drop_legacy_bindings(&landed, &extern_crates);
        if !names.is_empty() {
            landed = place_names(&landed, &names);
        }
        write_in_place(&path, landed.into_bytes(), &mut footprint)?;
    }
    // every rule of the spec rewrites the file it names, and every rule reads the file as the tree
    // wrote it: a rule that read a file an earlier rule had already written would search a text no
    // rule was written for, and a call the first rule landed is no longer the call the second rule
    // states. The places are read from one text and written once
    let mut rules_by_file: BTreeMap<PathBuf, Vec<&'static Rule>> = BTreeMap::new();
    for rule in spec.rules {
        let file = crate_src.join(rule.file);
        if !file.is_file() {
            missed.push(rule.id.to_string());
            continue;
        }
        rules_by_file.entry(file).or_default().push(rule);
    }
    for (file, rules) in rules_by_file {
        let mut current = normalize(&fs::read_to_string(&file)?);
        for rule in rules {
            let landed = match &rule.locate {
                Locate::Surround(surround) => match rule.hold_back_in {
                    Some(attribute) => crate::rules::rename::surround_outside(&current, surround, attribute),
                    None => crate::rules::rename::surround(&current, surround),
                },
                _ => crate::rules::Landed::NotApplicable,
            };
            match landed {
                crate::rules::Landed::Rewritten(text) => {
                    current = text;
                    applied.push(rule.id.to_string());
                }
                crate::rules::Landed::AnchorMissing | crate::rules::Landed::NotApplicable => {
                    missed.push(rule.id.to_string())
                }
            }
        }
        write_in_place(&file, current.into_bytes(), &mut footprint)?;
    }
    Ok(InjectResult {
        rules: (applied, missed),
        abandoned,
        footprint,
        lock: injection_lock,
    })
}

fn unreachable_landing(crate_src: &Path, rule: Option<&Rule>, why: &str) -> ! {
    match rule {
        Some(rule) => eprintln!(
            "[lvpa-wrapper] FATAL: rule {} found no landing in {}: {why}. A rule written for a crate \
             this pass covers must land, so the pass stops here.",
            rule.id,
            crate_src.display()
        ),
        None => eprintln!(
            "[lvpa-wrapper] FATAL: {why}. A seam written for a crate this pass covers either \
             replaces the edge it is declared against or stops the pass, and it stops here in {}.",
            crate_src.display()
        ),
    }
    exit(93);
}

pub fn replace_an_edge(
    manifest: &Path,
    dependency: &rules::SeamDependency,
    footprint: &mut Footprint,
) -> Result<(), String> {    let text = normalize(
        &fs::read_to_string(manifest)
            .map_err(|error| format!("{} is not readable: {error}", manifest.display()))?,
    );
    let target = normalized_package(dependency.package);
    let ours = dependency.path.trim_start_matches("crates/");
    // a card that states the edge by a path states a directory, and the directory named after the
    // last segment of the landing's path is the crate this repository holds; a path that merely
    // carries that segment somewhere inside its own name is another crate's directory
    let named = ours.rsplit('/').next().unwrap_or(ours);
    let reached = |line: &str| {
        let Some((_, value)) = line.split_once('=') else {
            return false;
        };
        // a card that states the edge by a path states another crate's directory, and that line is
        // the card's own word about where its dependency stands: the landing states a series and
        // never a path, so the line is left as it is
        if !value.contains("path") {
            return edge_package(line).is_some_and(|held| normalized_package(&held) == target);
        }
        // the card states this repository's crate by the one directory that holds it, and the line
        // is the landing's own line written against the root: it is rewritten against the card
        value
            .split('"')
            .nth(1)
            .is_some_and(|held| held.trim_end_matches('/').rsplit('/').next() == Some(named))
    };
    // the landing states the edge already, so the edge is stated once: a card that states it
    // twice is read by neither the resolver nor the reader
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut standing: Option<usize> = None;
    let mut replaced: Vec<String> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if !reached(line) {
            replaced.push(line.to_string());
            continue;
        }
        if standing.is_some() {
            continue;
        }
        standing = Some(index);
        let Some(cut) = line.find('=') else {
            replaced.push(line.to_string());
            continue;
        };
        let alias = line[..cut].trim_end();
        let ending = if line.ends_with('\n') { "\n" } else { "" };
        let inline = inline_dependency(dependency, manifest);
        let spec = match inline.split_once(" = ") {
            Some((_, spec)) => spec.trim_end().to_string(),
            None => inline,
        };
        replaced.push(format!("{alias} = {spec}{ending}"));
    }
    if standing.is_none() {
        return Err(format!(
            "{} carries no `{}` edge, and a seam replaces that edge rather than standing beside it",
            manifest.display(),
            dependency.declared_name()
        ));
    }
    let landed = replaced.concat();
    write_in_place(manifest, landed.into_bytes(), footprint)
        .map_err(|error| format!("{}: {error}", manifest.display()))?;
    Ok(())
}

/// The card read with one line rewritten, read as text and written nowhere: the line the tree
/// states for the package is put on the series the landing names, and every other line stands.
fn replace_an_edge_in(
    text: &str,
    dependency: &rules::SeamDependency,
    manifest: &Path,
) -> Result<String, String> {
    let target = normalized_package(dependency.package);
    let ours = dependency.path.trim_start_matches("crates/");
    let named = ours.rsplit('/').next().unwrap_or(ours);
    let reached = |line: &str| {
        let Some((_, value)) = line.split_once('=') else {
            return false;
        };
        if !value.contains("path") {
            return edge_package(line).is_some_and(|held| normalized_package(&held) == target);
        }
        value
            .split('"')
            .nth(1)
            .is_some_and(|held| held.trim_end_matches('/').rsplit('/').next() == Some(named))
    };
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut standing = false;
    let mut replaced: Vec<String> = Vec::new();
    for line in &lines {
        if !reached(line) {
            replaced.push(line.to_string());
            continue;
        }
        if standing {
            continue;
        }
        standing = true;
        let Some(cut) = line.find('=') else {
            replaced.push(line.to_string());
            continue;
        };
        let alias = line[..cut].trim_end();
        let ending = if line.ends_with('\n') { "\n" } else { "" };
        let inline = inline_dependency(dependency, manifest);
        let spec = match inline.split_once(" = ") {
            Some((_, spec)) => spec.trim_end().to_string(),
            None => inline,
        };
        replaced.push(format!("{alias} = {spec}{ending}"));
    }
    if !standing {
        return Err(format!(
            "{} carries no `{}` edge, and a seam replaces that edge rather than standing beside it",
            manifest.display(),
            dependency.declared_name()
        ));
    }
    Ok(replaced.concat())
}
fn manifest_spec_path(root: &Path, crate_dir: &Path) -> String {
    // a workspace-rooted card reads every path from the root that holds the workspace file, and
    // that root is above the tree directory the repository holds (`obf`) as well as above `crates`
    let spelled = if crate_dir.starts_with(root) {
        root.to_path_buf()
    } else {
        workspace_root_path().unwrap_or_else(|| root.to_path_buf())
    };
    let root = spelled.as_path();
    let relative = match crate_dir.strip_prefix(root) {
        Ok(relative) => relative.to_path_buf(),
        Err(_) => crate_dir.to_path_buf(),
    };
    let text = relative.display().to_string().replace('\\', "/");
    if text.is_empty() {
        ".".to_string()
    } else {
        text
    }
}

fn relative_manifest_path(crate_dir: &Path, manifest: &Path) -> String {
    let Some(manifest_dir) = manifest.parent() else {
        return crate_dir.display().to_string().replace('\\', "/");
    };
    let crate_dir = if crate_dir.is_absolute() {
        crate_dir.to_path_buf()
    } else {
        manifest_dir.join(crate_dir)
    };
    let crate_dir = crate_dir.as_path();
    let mut common = manifest_dir.components().count();
    while common > 0 {
        let prefix: PathBuf = manifest_dir.components().take(common).collect();
        if crate_dir.starts_with(&prefix) {
            break;
        }
        common -= 1;
    }
    let mut out = String::new();
    for _ in common..manifest_dir.components().count() {
        out.push_str("../");
    }
    let tail: PathBuf = crate_dir.components().skip(common).collect();
    out.push_str(&tail.display().to_string().replace('\\', "/"));
    if out.is_empty() {
        out.push('.');
    }
    out
}

/// A source statement this repository states, applied to the text the compiler reads: the
/// tree's file stands as the tree wrote it, and the compiler is handed the file with this one
/// statement carried out. The find text is read off the tree itself - a signature as it stands
/// - and a find that no longer matches is the pass stopping, not a rewrite quietly skipped.
pub fn carry_a_statement(path: &Path, find: &str, replace: &str) -> Result<bool, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{} cannot be read: {error}", path.display()))?;
    let count = text.matches(find).count();
    if count == 0 {
        return Ok(false);
    }
    if count != 1 {
        return Err(format!(
            "{} holds {count} of the statement this repository states for it, and the pass rewrites one",
            path.display()
        ));
    }
    let mut footprint = Footprint::default();
    write_in_place(path, text.replace(find, replace).into_bytes(), &mut footprint)
        .map_err(|error| format!("{} cannot carry the statement: {error}", path.display()))?;
    footprint.restore();
    Ok(true)
}

fn write_seam_module(
    src: &Path,
    crate_src: &Path,
    seam: &rules::Seam,
    footprint: &mut Footprint,
    _read_by: &'static [Rule],
) -> std::io::Result<Option<(&'static str, &'static [&'static str])>> {
    // the body is held in the host's crate, and the copy is written into the crate this pass
    // covers, because that is the crate whose sources the rule reads: a copy written anywhere
    // else is a copy the run leaves behind
    let module_dir = crate_src.join(seam.module);
    // a body handed in as a file lands as the module that file is: the crate root names the
    // module, and the file the root names carries it
    let flat_seeds: &'static [&'static str] = &["lib.rs"];
    let seeds = if seam.seeds.iter().any(|seed| seed.ends_with(".rs")) {
        seam.seeds
    } else {
        flat_seeds
    };
    let source = if src.extension().is_some_and(|ext| ext == "rs") { src.to_path_buf() } else {
        src.join("lib.rs")
    };
    let held = fs::read_to_string(&source)?;
    let first_source = source.clone();
    let root_source = crate_src.of_the_crate();
    // the module stands in the crate root itself, not in a file beside it: the line that states
    // it and the body that is it are the same source, so both are written into that one file
    // a module stated inside the crate root is named from there by a path, so the sources that
    // call it name the root's item and not a crate of its own
    if matches!(seam.seed, rules::SeamSeed::Module) {
        // a seam whose seed is a file of the crate is a module of that one file: the crate root
        // names it, and the file the root names is the body the pass reads
        if let Some(seed) = seeds.first().copied().filter(|seed| seed.ends_with(".rs")) {
            let root_before = fs::read_to_string(&root_source).unwrap_or_default();
            if declare_module(&root_source, seam.module, footprint).is_ok() {
                let stated = fs::read_to_string(&root_source).unwrap_or_default();
                let body = land_seam_source(&held, &source, seam.module)?;
                if stated != root_before {
                    write_in_place(&root_source, stated.into_bytes(), footprint)?;
                }
                let flat = crate_src.join(seed);
                write_in_place(&flat, body.into_bytes(), footprint)?;
                return Ok(Some((seam.module, seam.seeds)));
            }
        }
        let mut stated = match declare_module(&root_source, seam.module, footprint) {
            Ok(()) => fs::read_to_string(&root_source).unwrap_or_default(),
            Err(error) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("{}: {error}", root_source.display()),
                ))
            }
        };
        let body = land_seam_source(&held, &source, seam.module)?;
        if !state_the_body(&mut stated, &body, seam.module) {
            return Err(landing_miss(
                &root_source,
                seam.module,
                "the crate root carries no item the module body continues from",
            ));
        }
        let paths = land_whole_paths(&stated, seam.module);
        write_in_place(&root_source, paths.into_bytes(), footprint)?;
        return Ok(Some((seam.module, seam.seeds)));
    }
    let root_text = fs::read_to_string(&root_source).unwrap_or_default();
    let mut landed_one = landing_lists_a_seed(&held, seam.module)
        || landing_lists_a_seed(&root_text, seam.module);
    let body = match land_seam_source(&held, &source, seam.module) {
        Ok(body) => body,
        Err(error) => {
            return Err(error);
        }
    };
    if matches!(seam.seed, rules::SeamSeed::Module) {
        let one = module_dir.join("mod.rs");
        write_in_place(&one, body.clone().into_bytes(), footprint)?;
    } else {
        let crate_dir = source.clone();
        for entry in fs::read_dir(&crate_dir)?.flatten() {
            let path = entry.path();
            if !path.is_file() || !path.extension().is_some_and(|ext| ext == "rs") {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()).map(str::to_string) else {
                continue;
            };
            let landed = module_dir.join(&name);
            if path == first_source {
                write_in_place(&landed, body.clone().into_bytes(), footprint)?;
                continue;
            }
            let sibling = fs::read_to_string(&path)?;
            landed_one = landed_one || landing_lists_a_seed(&sibling, seam.module);
            let own = land_seam_source(&sibling, &path, seam.module)?;
            write_in_place(&landed, own.into_bytes(), footprint)?;
        }
    }
    if landed_one {
        Ok(Some((seam.module, seam.seeds)))
    } else {
        Err(landing_miss(
            &source,
            seam.module,
            "no source of the covered crate names the seam module, so the module is written where nothing reaches it",
        ))
    }
}

fn seam_module_paths(
    crate_src: &Path,
    declared: &[(&'static str, &'static [&'static str])],
) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    for (module, seeds) in declared {
        for seed_at in *seeds {
            let anchor = crate_src.join(seed_at);
            if let Some(dir) = anchor.parent() {
                let landed = dir.join(module).join("mod.rs");
                if landed.is_file() && !roots.contains(&landed) {
                    roots.push(landed);
                }
            }
        }
    }
    roots
}

/// A seam module landing states the edge the host calls it by: the host either already carries
/// that package, and the line it carries is rewritten to point at the crate this repository
/// holds, or it carries no line for it and one is stated. The card ends with one line for the
/// package either way, and the landing is read from and written to the card of one crate.
fn state_the_edge(
    text: &str,
    dependency: &rules::SeamDependency,
    manifest: &Path,
    crate_src: &Path,
) -> Result<String, String> {
    let _ = crate_src;
    if !declares_edge(text, dependency) {
        let line = manifest_edge_line(dependency, manifest);
        let at = section_end(text, "dependencies");
        let mut landed = String::with_capacity(text.len() + line.len() + 1);
        landed.push_str(&text[..at]);
        landed.push_str(&line);
        landed.push('\n');
        landed.push_str(&text[at..]);
        return Ok(landed);
    }
    if dependency.stands_down {
        return Ok(stand_the_edge_down_in(text, dependency));
    }
    if let Some(series) = dependency.version {
        let landed = replace_an_edge_at_series(text, dependency, series)?;
        if landed != text {
            return Ok(landed);
        }
    }
    replace_an_edge_in(text, dependency, manifest)
}

/// The card read with one line put on a series, read as text: a line the card states by the key the
/// landing names - a bare series or a table of its own - stands on the series the landing states,
/// and a line the card states for another package stands as the tree wrote it.
fn replace_an_edge_at_series(
    text: &str,
    dependency: &rules::SeamDependency,
    series: &str,
) -> Result<String, String> {
    let key = dependency.declared_name();
    let named: &str = &key;
    let mut out = String::with_capacity(text.len());
    let mut standing = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        let at_key = !trimmed.starts_with(['#', '['])
            && crate::edges::edge_key_of(line) == Some(named)
            && !trimmed.contains("path =");
        if !at_key {
            out.push_str(line);
            continue;
        }
        if standing {
            continue;
        }
        standing = true;
        let features = crate::edges::features_of(line);
        let carried = crate::edges::rest_of(line);
        let ending = if line.ends_with('\n') { "\n" } else { "" };
        out.push_str(&format!("{}{ending}", crate::edges::single_line(named, series, &carried, &features)));
    }
    if !standing {
        return Err(format!("the card states no `{key}` line to land"));
    }
    Ok(out)
}


/// The card read down at one line, read as text: the line the tree states for the package is taken
/// out and every other line stands as the tree wrote it.
fn stand_the_edge_down_in(text: &str, dependency: &rules::SeamDependency) -> String {
    let target = normalized_package(dependency.package);
    let mut taken = false;
    let mut kept = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        if !taken && edge_package(line).is_some_and(|held| normalized_package(&held) == target) {
            taken = true;
            continue;
        }
        kept.push_str(line);
    }
    kept
}

fn manifest_edge_line(dependency: &rules::SeamDependency, manifest: &Path) -> String {
    let key = dependency.declared_name().replace('-', "_");
    let origin = dependency.origin(&inline_origin(dependency, manifest));
    let mut spec = format!("{{ package = \"{}\", {origin}", dependency.package);
    if !dependency.features.is_empty() {
        let features: Vec<String> = dependency.features.iter().map(|feature| format!("\"{feature}\"")).collect();
        spec.push_str(&format!(", features = [{}]", features.join(", ")));
    }
    spec.push('}');
    format!("{key} = {spec}")
}
fn section_end(text: &str, section: &str) -> usize {
    let header = format!("[{section}]");
    let mut bodies: Vec<(usize, &str)> = Vec::new();
    let mut at = 0usize;
    for line in text.split_inclusive('\n') {
        bodies.push((at, line));
        at += line.len();
    }
    let mut found: Option<usize> = None;
    let mut depth = 0usize;
    let mut index = 0usize;
    while index < bodies.len() {
        let (start, line) = bodies[index];
        index += 1;
        let body = line.trim();
        if depth == 0 {
            // the section is read as a header of its own line: the first line of a card is the
            // opening of its first section, and a header found inside a longer key names no
            // section at all
            if body.starts_with('[') {
                if found.is_some() {
                    return start;
                }
                if body == header {
                    found = Some(start);
                }
                continue;
            }
            if found.is_none() {
                continue;
            }
        }
        depth = depth.saturating_add(line.matches('{').count());
        depth = depth.saturating_sub(line.matches('}').count());
    }
    if found.is_some() {
        return text.len();
    }
    text.len()
}

fn declare_module(
    anchor: &Path,
    module: &str,
    footprint: &mut Footprint,
) -> std::io::Result<()> {
    let Ok(text) = fs::read_to_string(anchor) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!(
                "{} is not a file the crate states its modules in, so `mod {module}` has no line to go on",
                anchor.display()
            ),
        ));
    };
    let text = normalize(&text);
    if module_line_in(&text, module) {
        return Ok(());
    }
    let at = declaration_after_inner_attributes(&text);
    let mut landed = String::with_capacity(text.len() + module.len() + 8);
    landed.push_str(&text[..at]);
    if at > 0 && !text[..at].ends_with('\n') {
        landed.push('\n');
    }
    landed.push_str(&format!("pub mod {module};\n"));
    landed.push_str(&text[at..]);
    write_in_place(anchor, landed.into_bytes(), footprint)?;
    Ok(())
}

/// Whether a line of this text declares the module: read off the text rather than off a parse, the
/// module the next write lands need not parse yet.
fn module_line_in(text: &str, module: &str) -> bool {
    for line in text.lines() {
        let body = line.trim_start();
        let rest = body
            .strip_prefix("pub mod ")
            .or_else(|| body.strip_prefix("mod "))
            .unwrap_or("");
        let name: String = rest
            .chars()
            .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
            .collect();
        let tail = rest[name.len()..].trim_start();
        if name == module && (tail.starts_with(';') || tail.starts_with('{')) {
            return true;
        }
    }
    false
}

fn declaration_after_inner_attributes(text: &str) -> usize {
    let mut at = 0usize;
    while at < text.len() {
        let end = match text[at..].find('\n') {
            Some(offset) => at + offset + 1,
            None => text.len(),
        };
        let body = text[at..end].trim_start_matches('\u{feff}').trim();
        // the first line of a file is where its inner doc comments are; a byte order mark or a
        // blank line ahead of them does not move that line out from under them
        if body.starts_with("//!") || body.starts_with("#![") {
            at = end;
            continue;
        }
        if body.is_empty() && at == 0 {
            at = end;
            continue;
        }
        break;
    }
    at
}

fn declare_extern_crates(text: &str, crates: &[String]) -> String {
    // the crate the module is compiled in holds the seam crate as a dependency, and the module
    // is where that crate is named: the binding goes in the module, not in the crate root
    // a byte order mark belongs at the head of a file and nowhere else: a statement put above it
    // ends the file's first token before it starts, so it comes after the mark
    // the doc comments the body opens with stay its first statement: the binding stands under
    // them, where the body's own statements begin
    let held: Vec<&str> = text.split_inclusive('\n').collect();
    let opened = held
        .iter()
        .take_while(|line| {
            let body = line.trim_start().trim_start_matches('\u{feff}');
            body.starts_with("//!") || body.trim().is_empty()
        })
        .count();
    // a binding an earlier pass wrote stands where this pass puts it, above the doc comments the
    // body opens with: the mark and the comments before it are the head, and the binding follows
    // the mark stands at the head of the file and nowhere else, so the bindings come after it and
    // before the doc comments the body opens with: those two are one statement, not two
    let mut head = 0usize;
    while head < opened {
        let body = held[head].trim_start_matches('\u{feff}').trim();
        if body.starts_with("//!") || body.starts_with("#![") {
            head += 1;
            continue;
        }
        break;
    }
    let reused: Vec<&str> = held[head..opened]
        .iter()
        .copied()
        .filter(|line| crates.iter().any(|name| line.trim() == format!("extern crate {name};")))
        .collect();
    let cut = head;
    let mut out_head = String::with_capacity(text.len() + crates.len() * 24);
    for line in &held[..head] {
        out_head.push_str(line);
    }
    for line in &reused {
        out_head.push_str(line);
    }
    for name in crates {
        if !reused.iter().any(|line| line.trim() == format!("extern crate {name};")) {
            out_head.push_str(&format!("extern crate {name};\n"));
        }
    }
    let mut out = out_head;
    for line in &held[cut..] {
        out.push_str(line);
    }
    out
}

fn drop_legacy_bindings(text: &str, crates: &[String]) -> String {
    // a statement of the body's submodules is what names them: the body is the module, so its
    // own submodules are reached by the paths it already writes
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim();
        let names_a_module = body
            .strip_prefix("pub mod ")
            .or_else(|| body.strip_prefix("mod "))
            .and_then(|rest| rest.strip_suffix(';'))
            .is_some_and(|rest| !rest.trim().is_empty() && crates.iter().all(|name| rest.trim() != name));
        if names_a_module {
            continue;
        }
        out.push_str(line);
    }
    out
}
fn landing_miss(source: &Path, module: &str, detail: &str) -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!(
            "seam {module} cannot be landed from {}: {detail}",
            source.display()
        ),
    )
}

fn land_seam_source(text: &str, source: &Path, module: &str) -> std::io::Result<String> {
    let text = strip_seam_tests(text);
    let text = &text;
    let crlf = text.contains("\r\n");
    let lf = text.replace("\r\n", "\n");
    let probe = drop_own_module_declaration(&lf, module);
    if let Err(error) = syn::parse_file(&probe) {
        return Err(landing_miss(
            source,
            module,
            &format!("the seam source does not parse: {error}"),
        ));
    }
    let landed = reanchor_seam_crate_paths(&probe, module);
    Ok(if crlf {
        landed.replace('\n', "\r\n")
    } else {
        landed
    })
}
/// The tests a seam crate keeps beside its body are read by that crate and by no other: the module
/// the crate carries states the body, and a block of tests stated twice is a block of tests that
/// runs twice.
fn strip_seam_tests(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0usize;
    let mut search = 0usize;
    while let Some(found) = text[search..].find("#[cfg(test)]") {
        let start = search + found;
        if start > 0 && !text[..start].ends_with('\n') {
            search = start + 1;
            continue;
        }
        let Some(open) = text[start..].find('{').map(|offset| start + offset) else {
            break;
        };
        let mut depth = 0i64;
        let mut end = None;
        for (offset, ch) in text[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(open + offset + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            break;
        };
        out.push_str(&text[at..start]);
        at = end;
        search = end;
    }
    out.push_str(&text[at..]);
    out
}

fn state_the_body(text: &mut String, body: &str, module: &str) -> bool {
    // the module stands as an inline module of the crate root, placed after the items the root
    // already states: the body it carries is the body the seam crate holds, and each pass states
    // it once, so the body a pass before this one wrote is taken out before this one goes in
    let stated = format!("pub mod {module};");
    let inline = format!("pub mod {module} {{\n");
    // this pass states the module once: the lines a pass before it left for it are the lines it
    // wrote, and the root is read without them -- and so is every line that closes after them,
    // because those are the closings of items that pass left open
    let mut without_line = text.clone();
    while let Some(closed) = closing_line(&without_line, module) {
        without_line = format!("{}{}", &without_line[..closed.open_start], &without_line[closed.after_end..]);
    }

    let without_line: String = without_line
        .lines()
        .filter(|line| line.trim() != stated)
        .map(|line| format!("{line}\n"))
        .collect();
    let mut landed = without_line;
    if !landed.ends_with('\n') && !landed.is_empty() {
        landed.push('\n');
    }
    landed.push_str(&inline);
    landed.push('\n');
    landed.push_str(body.trim_start_matches('\u{feff}'));
    if !landed.ends_with('\n') {
        landed.push('\n');
    }
    landed.push_str("}\n");
    *text = landed;
    true
}

fn land_whole_paths(text: &str, module: &str) -> String {
    // the module stands inside the crate root, so a path to it stands inside an item's body and
    // not at a file's head: every such path is read as a path of this crate, wherever it stands
    let mut out = String::with_capacity(text.len() + 64);
    let mut at = 0usize;
    let needle = format!("{module}::");
    let mut depth = 0i64;
    let mut scanned = 0usize;
    while let Some(found) = text[at..].find(&needle) {
        let start = at + found;
        for ch in text[scanned..start].chars() {
            match ch {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
        scanned = start;
        out.push_str(&text[at..start]);
        // a path stands in a body wherever the braces before it are still open: at a file's head
        // a name is a crate of its own, and inside an item it is an item of this crate
        if depth > 0 {
            out.push_str("crate::");
        }
        out.push_str(&needle);
        at = start + needle.len();
    }
    out.push_str(&text[at..]);
    out
}

struct Closed {
    open_start: usize,
    after_end: usize,
}

fn closing_line(text: &str, module: &str) -> Option<Closed> {
    // the statement a pass wrote stands as one line opening the module and the line closing it at
    // the head of a line: what lies between them is the body that pass wrote, and nothing else
    let opener = format!("pub mod {module} {{");
    let mut at = 0usize;
    let mut open_start: Option<usize> = None;
    for line in text.split_inclusive('\n') {
        let body = line.trim();
        if open_start.is_none() {
            if body.starts_with(&opener) {
                open_start = Some(at);
            }
        } else if body.starts_with('}') {
            return Some(Closed {
                open_start: open_start?,
                after_end: at + line.len(),
            });
        }
        at += line.len();
    }
    None
}

fn landing_lists_a_seed(text: &str, module: &str) -> bool {
    text.lines().any(|line| {
        let body = line.trim();
        body == format!("pub mod {module};") || body == format!("mod {module};")
    })
}

fn drop_own_module_declaration(text: &str, module: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let body = line.trim();
        if body == format!("pub mod {module};") || body == format!("mod {module};") {
            continue;
        }
        out.push_str(line);
    }
    out
}

fn reanchor_seam_crate_paths(text: &str, module: &str) -> String {
    // the body stands as a module of the crate this pass covers, so it states the crate's items
    // the way a module of that crate does: the paths the body writes as `crate::` name the crate
    // the body is read out of, and those items are reached through the crate root this module
    // stands under. A path to a crate by its own name is a path of the crate root, so it is
    // stated from there rather than from inside this module.
    let mut out = String::with_capacity(text.len() + 64);
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("//") {
            out.push_str(line);
            continue;
        }
        // a crate the body reaches is reached from the root of the crate this module stands in, and
        // that name is stated once on the way in: a path that already names the root is left as it
        // stands, so the crate is not counted for twice
        let mut swapped = line.replace("crate::", "super::");
        for (from, to) in reach_of_the_root() {
            swapped = swapped.replace(&format!("crate::{from}"), &to);
            if !to.starts_with("crate::") {
                swapped = swapped.replace(&from, &to);
            }
        }
        out.push_str(&swapped);
    }
    let _ = module;
    out
}

/// The crates the seam body names by their own name: a module of a crate does not name the crate
/// itself, so each of these is reached from the root of the crate this module stands in.
fn reach_of_the_root() -> Vec<(String, String)> {
    vec![
        ("sqlx_sqlite::".to_string(), "crate::sqlx_sqlite::".to_string()),
        ("libsqlite3_sys::".to_string(), "crate::libsqlite3_sys::".to_string()),
    ]
}

pub fn place_names(module_text: &str, names: &str) -> String {
    let stated = names.trim_end_matches('\n');
    let held: Vec<&str> = module_text.split_inclusive('\n').collect();
    // a statement stands after the doc comments the body opens with and after the bindings the
    // crate root holds: the head of a body is its comments, and the statement belongs under them
    let opened = held
        .iter()
        .take_while(|line| {
            let body = line.trim_start_matches('\u{feff}').trim();
            body.starts_with("//!") || body.starts_with("#![") || body.starts_with("extern crate ") || body.is_empty()
        })
        .count();
    if held.iter().any(|line| line.trim() == stated) {
        return module_text.to_string();
    }
    let mut out = String::with_capacity(module_text.len() + names.len());
    for line in &held[..opened] {
        out.push_str(line);
    }
    out.push_str(names);
    for line in &held[opened..] {
        out.push_str(line);
    }
    out
}

fn write_in_place(path: &Path, bytes: Vec<u8>, footprint: &mut Footprint) -> std::io::Result<()> {
    save_before_write(path, footprint)?;
    let mut file = fs::File::create(path)?;
    std::io::Write::write_all(&mut file, &bytes)?;
    std::io::Write::flush(&mut file)?;
    drop(file);
    let held = fs::read(path).unwrap_or_default();
    if held != bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "{} holds {} bytes right after {} were written, so the tree under it is not the file this pass wrote",
                path.display(),
                held.len(),
                bytes.len()
            ),
        ));
    }
    Ok(())
}

pub fn save_before_write(path: &Path, footprint: &mut Footprint) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
    if footprint.files.iter().any(|entry| entry.path == path) {
        return Ok(());
    }
        create_dir_recording(dir, footprint)?;
    }
    if footprint.files.iter().any(|saved| saved.path == path) {
        return Ok(());
    }
    match fs::read(path) {
        Ok(original) => footprint.files.push(Restored {
            path: path.to_path_buf(),
            original: Some(original),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            footprint.files.push(Restored {
                path: path.to_path_buf(),
                original: None,
            });
        }
        Err(error) => return Err(error),
    }
    refresh_live_record(footprint)
}

fn create_dir_recording(dir: &Path, footprint: &mut Footprint) -> std::io::Result<()> {
    // every directory on the way in is recorded, including one that already stands: a statement
    // written into a directory the pass did not make is a statement an earlier interrupted run
    // left, and the record is what lets the next run take it back
    let mut walked: Vec<&Path> = Vec::new();
    let mut cursor = Some(dir);
    while let Some(path) = cursor {
        if footprint.dirs.iter().any(|held| held == path) {
            break;
        }
        walked.push(path);
        cursor = path.parent();
    }
    for path in walked.iter().rev() {
        if !path.is_dir() {
            fs::create_dir(path)?;
        }
        // the record holds every directory under the crate, standing or made: a module written
        // into a directory an earlier run made is a module the next run takes back by that record
        if path.starts_with(dir) && !footprint.dirs.iter().any(|held| held == path) {
            footprint.dirs.push((*path).to_path_buf());
        }
    }
    Ok(())
}

fn strip_unc(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path,
    }
}

pub enum Resolution {
    Rewritten(String),
    NotApplicable,
    AnchorMissing,
}

pub struct Applied {
    pub text: String,
    pub applied: bool,
}

pub fn rewrite_text(
    path: &Path,
    rule: &Rule,
    previous: Option<&str>,
) -> std::io::Result<Resolution> {
    resolve(path, rule, previous)
}

/// Every statement the tables state is carried to the text its series reads. A line the tree
/// stopped writing is a line this pass has nothing to carry, and one that moved by a word is the
/// pass stopping: the crate would otherwise compile at a series its own source does not reach.
fn carry_every_statement(report: &mut Report, crate_root: &Path, src: &Path) {
    let Some(spec) = rules::spec_of_crate_root(crate_root) else {
        return;
    };
    for carry in spec.carries {
        let path = src.join(carry.file);
        match carry_a_statement(&path, carry.from, carry.to) {
            Ok(true) => report.rewrote.push(format!("{}: {}", carry.file, carry.id)),
            Ok(false) => {}
            Err(error) => {
                eprintln!("[lvpa-wrapper] FATAL: {error}");
                restore_then_exit(report, 93);
            }
        }
    }
}

pub fn apply_rule(
    path: &Path,
    rule: &Rule,
    previous: Option<&str>,
) -> std::io::Result<Applied> {
    let resolved = resolve(path, rule, previous)?;
    Ok(match resolved {
        Resolution::Rewritten(text) => Applied {
            text,
            applied: true,
        },
        Resolution::NotApplicable | Resolution::AnchorMissing => Applied {
            text: read_source(path, previous)?,
            applied: false,
        },
    })
}

fn read_source(path: &Path, previous: Option<&str>) -> std::io::Result<String> {
    match previous {
        Some(text) => Ok(text.to_string()),
        None => fs::read_to_string(path),
    }
}

pub fn edge_key_of(line: &str) -> Option<&str> {
    let name = line.split('=').next()?.trim();
    let name = name.split('.').next()?.trim();
    if name.is_empty() || !name.chars().all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '-') {
        return None;
    }
    Some(name)
}

fn resolve(
    path: &Path,
    rule: &Rule,
    previous: Option<&str>,
) -> std::io::Result<Resolution> {
    let source = read_source(path, previous)?;
    let crlf = source.contains("\r\n");
    let current = normalize(&source);

    let resolved = match &rule.locate {
        Locate::Composition(composition) => {
            let crate::rules::composition::Composition::Envelope {
                envelope,
                entry,
                ..
            } = composition;
            let seam = match rule_seam(rule) {
                Some(seam) => seam,
                None => unreachable_landing(
                    crate_src_of(path),
                    Some(rule),
                    "the seam it is written for is not in the rule table",
                ),
            };
            let carried = match workspace_root_path() {
                Some(root) => root.join(seam.host.as_str_checked()).to_string_lossy().into_owned(),
                None => unreachable_landing(
                    crate_src_of(path),
                    Some(rule),
                    "the workspace root did not resolve, so the seam body cannot be read",
                ),
            };
            match crate::rules::composition::wrap(&current, &carried, envelope, entry) {
                crate::semantic::Nesting::Landed(composed) => {
                    crate::rules::Landed::Rewritten(composed.text)
                }
                crate::semantic::Nesting::NoSuchSeam => crate::rules::Landed::NotApplicable,
                crate::semantic::Nesting::AnchorGone(_) => crate::rules::Landed::AnchorMissing,
            }
        }
        Locate::Completion(completion) => {
            let exports = workspace_root_path()
                .and_then(|root| fs::read_to_string(root.join(completion.exports)).ok());
            match exports {
                Some(exports) => crate::rules::completion::wrap(&current, completion, &exports),
                None => crate::rules::Landed::AnchorMissing,
            }
        }
        Locate::Rename(rename) => crate::rules::rename::rename(&current, rename),
        Locate::Surround(surround) => crate::rules::rename::surround(&current, surround),
    };

    Ok(match resolved {
        crate::rules::Landed::Rewritten(rewritten) => Resolution::Rewritten(if crlf {
            rewritten.replace('\n', "\r\n")
        } else {
            rewritten
        }),
        crate::rules::Landed::NotApplicable => Resolution::NotApplicable,
        crate::rules::Landed::AnchorMissing => Resolution::AnchorMissing,
    })
}

fn crate_src_of(path: &Path) -> &Path {
    path.parent().unwrap_or(path)
}

trait RootOfTheCrate {
    fn of_the_crate(&self) -> PathBuf;
}

impl RootOfTheCrate for Path {
    fn of_the_crate(&self) -> PathBuf {
        // after the take the crate root is a crate of the mirror, not the tree's directory: a
        // single-crate tree hands in the crate's own source directory, and the root stands there
        let at = match self.file_name().and_then(|name| name.to_str()) {
            Some("src") => self.to_path_buf(),
            _ => self.join("src"),
        };
        let flat = at.join("lib.rs");
        if flat.is_file() {
            flat
        } else {
            at.join("lib").join("mod.rs")
        }
    }
}

fn rule_seam(rule: &Rule) -> Option<&'static rules::Seam> {
    let module = rule_seam_module(rule)?;
    rules::registered()
        .into_iter()
        .flat_map(|spec| spec.seams.iter())
        .find(|seam| seam.module == module)
}

fn rule_seam_module(rule: &Rule) -> Option<&'static str> {
    match &rule.locate {
        Locate::Composition(crate::rules::composition::Composition::Envelope { seam, .. }) => {
            Some(seam)
        }
        Locate::Completion(completion) => Some(completion.seam),
        Locate::Rename(_) => None,
        Locate::Surround(surround) => Some(surround.seam),
    }
}

pub fn modules_of(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let body = line.trim_start();
        let rest = body
            .strip_prefix("pub mod ")
            .or_else(|| body.strip_prefix("mod "))
            .unwrap_or("");
        let name: String = rest
            .chars()
            .take_while(|ch| ch.is_alphanumeric() || *ch == '_')
            .collect();
        let tail = &rest[name.len()..];
        if !name.is_empty() && (tail.trim_start().starts_with(';') || tail.trim_start().starts_with('{')) {
            out.push(name);
        }
    }
    out
}

fn normalized_package(name: &str) -> String {
    name.replace('-', "_").to_ascii_lowercase()
}
fn edge_key(line: &str) -> Option<&str> {
    let cut = line.find('=')?;
    let alias = line[..cut].trim();
    if alias.is_empty()
        || alias.starts_with('[')
        || alias.starts_with('#')
        || alias.contains(char::is_whitespace)
    {
        return None;
    }
    Some(alias)
}
fn edge_package(line: &str) -> Option<String> {
    let key = edge_key(line)?;
    let rest = line[key.len()..].trim_start().strip_prefix('=')?;
    let Some((_, after)) = rest.split_once("package") else {
        return Some(key.to_string());
    };
    let quoted = after.trim_start().strip_prefix('=')?.trim_start();
    match quoted.chars().next() {
        Some(quote @ ('"' | '\'')) => quoted[1..].split(quote).next().map(str::to_string),
        _ => Some(key.to_string()),
    }
}
fn declares_edge(text: &str, dependency: &rules::SeamDependency) -> bool {
    // the landing states a series and never a path, so a line the card states by a path names the
    // directory this repository holds and is the landing's own line; a package states the path it
    // reaches, and a path is the directory named after the last segment of it
    let target = normalized_package(dependency.package);
    let ours = dependency.path.trim_start_matches("crates/");
    let named = ours.rsplit('/').next().unwrap_or(ours);
    text.lines().any(|line| {
        let Some((_, value)) = line.split_once('=') else {
            return false;
        };
        if !value.contains("path") {
            return edge_package(line).is_some_and(|held| normalized_package(&held) == target);
        }
        value
            .split('"')
            .nth(1)
            .is_some_and(|held| held.trim_end_matches('/').rsplit('/').next() == Some(named))
    })
}
fn inline_dependency(dependency: &rules::SeamDependency, manifest: &Path) -> String {
    dependency.line(&inline_origin(dependency, manifest))
}

/// Where an edge points, stated the one way every card of this workspace reads it: a card nested
/// in an absorbed tree reads its paths against the workspace root, so a crate of this repository
/// is named from the root rather than from the card that carries the line.
fn inline_origin(dependency: &rules::SeamDependency, manifest: &Path) -> String {
    let derived = workspace_root_path().map(|root| root.join(dependency.path));
    let landed = derived
        .as_ref()
        .filter(|path| path.is_file() || path.join("Cargo.toml").is_file());
    let crate_dir = match landed {
        Some(landed) if landed.join("Cargo.toml").is_file() => landed.clone(),
        Some(file) => file.parent().map(Path::to_path_buf).unwrap_or_else(|| file.clone()),
        None => match derived {
            Some(path) => path,
            None => match manifest.parent() {
                Some(dir) => dir.join(dependency.path),
                None => PathBuf::from(dependency.path),
            },
        },
    };
    if dependency.path.starts_with('.') {
        dependency.path.to_string()
    } else if dependency.package.starts_with("lvpa-") {
        // the crate this repository holds is named by the directory the landing states it in, read
        // against the card that carries the line: a card nested in the tree reads a path from where
        // it stands, and the directory is one the card reaches by going up
        let root = workspace_root_path().unwrap_or_else(|| crate_dir.clone());
        let named = root.join(dependency.path);
        let named = if named.join("Cargo.toml").is_file() { named } else { crate_dir.clone() };
        relative_manifest_path(named.as_path(), manifest)
    } else {
        match workspace_root_path() {
            Some(root) => manifest_spec_path(root.as_path(), crate_dir.as_path()),
            None => relative_manifest_path(crate_dir.as_path(), manifest),
        }
    }
}
pub fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn crate_root_above(host: &Path) -> Option<&Path> {
    host.ancestors()
        .skip(1)
        .find(|at| at.join("Cargo.toml").is_file())
}

/// Every seam of every card this table names, laid into the crate it reaches. A crate handed to
/// the compiler is one of them, and the crates that were already built and cached never are:
/// the pass that stands them all is the one that runs before the graph is read.
pub fn lay_every_seam() {
    let Some(root) = crate::sources::workspace_root() else {
        return;
    };
    for spec in rules::registered() {
        // the file a seam lands into is a file of the crate the seam serves, and it is written once:
        // a crate both passes name is served by the seams of one statement of it, and the module the
        // crate root names is the one the last write leaves. The seam of the crate is the first one
        // stated for it that names that module.
        let mut laid: Vec<&'static rules::Seam> = Vec::new();
        for seam in spec.seams {
            let host = root.join(seam.host.as_str_checked());
            if laid.iter().any(|held| held.module == seam.module && same_crate(held, seam, &root)) {
                continue;
            }
            if let Err(error) = lay_seam(&host, seam) {
                eprintln!("[lvpa-wrapper] FATAL: {error}");
                std::process::exit(92);
            }
            laid.push(seam);
        }
    }
}
/// Two seams serve one crate where the crate root their hosts stand under is the same directory:
/// the body of a seam is the file its host names, and the crate root the file stands in is the one
/// crate a seam lands in, so two hosts in one crate are one landing rather than two.
fn same_crate(left: &rules::Seam, right: &rules::Seam, root: &Path) -> bool {
    match (crate_root_above(&root.join(left.host.as_str_checked())), crate_root_above(&root.join(right.host.as_str_checked()))) {
        (Some(one), Some(two)) => one == two,
        _ => false,
    }
}
pub fn lay_seam(host: &Path, seam: &'static rules::Seam) -> Result<(), String> {
    // the body is handed in from the seam crate, and the file it is handed into is the host's own:
    // the module stands beside the host source as the file the host root names for it
    let Some(crate_root) = crate_root_above(host) else {
        return Err(format!("{} names no crate root to lay the seam in", host.display()));
    };
    let at = if host.is_dir() {
        host.join("src")
    } else if host.is_file() {
        host.parent().map(Path::to_path_buf).unwrap_or_else(|| host.to_path_buf())
    } else {
        crate_root.join("src")
    };
    // a crate root names the module it takes, and the statement it takes it by names the file
    let seed = seam
        .seeds
        .first()
        .map(|seed| if seed.ends_with(".rs") { seed.to_string() } else { format!("{seed}.rs") })
        .ok_or_else(|| format!("seam {} names no module of the crate it lands in", seam.module))?;
    let source = crate::sources::workspace_root()
        .ok_or_else(|| "the workspace root could not be resolved".to_string())?
        .join(seam.body.as_str_checked());
    let held = if source.extension().is_some_and(|ext| ext == "rs") { source } else { source.join("lib.rs") };
    let read = fs::read_to_string(&held).map_err(|error| error.to_string())?;
    let body = land_seam_source(&read, &held, seam.module)
        .map_err(|error| error.to_string())?;
    if !at.is_dir() {
        fs::create_dir_all(&at).map_err(|error| error.to_string())?;
    }
    let mut footprint = Footprint::default();
    write_in_place(&at.join(&seed), body.into_bytes(), &mut footprint)
        .map_err(|error| format!("{} cannot carry seam {}: {error}", crate_root.display(), seam.module))?;
    footprint.restore();
    Ok(())
}

pub fn restore(report: &Report) {
    for entry in &report.footprint.files {
        match &entry.original {
            Some(bytes) => {
                let _ = fs::write(&entry.path, bytes);
            }
            None => {
                let _ = fs::remove_file(&entry.path);
            }
        }
    }
    for dir in report.footprint.dirs.iter().rev() {
        let _ = fs::remove_dir(dir);
    }
    if let Some(record) = &report.record {
        retire_record(record);
    }
    release_injection_locks();
}

fn release_injection_locks() {
    let mut held: Vec<InjectionLock> = LIVE_LOCKS.with(|cell| std::mem::take(&mut *cell.borrow_mut()));
    for lock in held.iter_mut() {
        lock.release();
    }
}

fn restore_then_exit(report: &Report, code: i32) -> ! {
    restore(report);
    exit(code);
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedFile {
    path: PathBuf,
    original: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedRun {
    files: Vec<SavedFile>,
    dirs: Vec<PathBuf>,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum OwnerState {
    Alive,
    Gone,
    Unknown,
}

fn record_owner_state(record: &Path) -> OwnerState {
    let Some(lock) = record_lock_path(record) else {
        return OwnerState::Gone;
    };
    match fs::OpenOptions::new().read(true).write(true).open(&lock) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => OwnerState::Gone,
        opened => classify_lock(opened),
    }
}

fn classify_settled(record: &Path) -> OwnerState {
    let mut readings = 0u32;
    let mut gone = 0u32;
    loop {
        readings += 1;
        let here = record_owner_state(record);
        if here == OwnerState::Gone {
            gone += 1;
            if gone >= SETTLE_ABSENCE {
                return OwnerState::Gone;
            }
        } else if here == OwnerState::Alive {
            return OwnerState::Alive;
        }
        if readings >= SETTLE_READINGS {
            return OwnerState::Unknown;
        }
        std::thread::yield_now();
    }
}

fn classify_lock(opened: std::io::Result<fs::File>) -> OwnerState {
    match opened {
        Ok(file) if file.try_lock().is_ok() => OwnerState::Gone,
        Ok(_) => OwnerState::Alive,
        Err(_) => OwnerState::Unknown,
    }
}

fn record_lock_path(record: &Path) -> Option<PathBuf> {
    let stem = record.file_stem()?;
    let mut name = stem.to_os_string();
    name.push(SEAM_RUN_LOCK_SUFFIX);
    record.parent().map(|dir| dir.join(name))
}

fn run_record_and_lock(dir: &Path) -> (PathBuf, PathBuf) {
    let record = dir.join(record_name());
    let lock = record_lock_path(&record)
        .expect("a record built from a name this module wrote has a stem, so it has a companion");
    (record, lock)
}

fn record_staging_path(record: &Path) -> Option<PathBuf> {
    let name = record.file_name()?;
    let mut staging = name.to_os_string();
    staging.push(SEAM_RUN_STAGING_SUFFIX);
    record.parent().map(|dir| dir.join(staging))
}

fn retire_record(record: &Path) {
    if let Some(staging) = record_staging_path(record) {
        if fs::rename(record, &staging).is_ok() {
            let _ = fs::remove_file(&staging);
        } else {
            let _ = fs::remove_file(record);
        }
    }
    if let Some(lock) = record_lock_path(record) {
        let _ = fs::remove_file(lock);
    }
}

fn take_run_lock(dir: &Path) -> std::io::Result<()> {
    let (_, path) = run_record_and_lock(dir);
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    file.lock()?;
    std::mem::forget(file);
    Ok(())
}

fn take_injection_lock(crate_src: &Path) -> std::io::Result<InjectionLock> {
    let path = injection_lock_path(crate_src).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the record directory could not be resolved, so no lock can be placed for this crate",
        )
    })?;
    if let Some(root) = path.parent() {
        fs::create_dir_all(root)?;
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    file.lock()?;
    Ok(InjectionLock {
        path: Some(path),
        file: Some(file),
    })
}

fn injection_lock_path(crate_src: &Path) -> Option<PathBuf> {
    let key = crate_key(crate_src);
    let name = format!("{SEAM_INJECTION_LOCK}{key}{SEAM_RUN_LOCK_SUFFIX}");
    seam_record_dir().map(|root| root.join(name))
}

fn crate_key(crate_src: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    crate_src.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

struct InjectionLock {
    path: Option<PathBuf>,
    file: Option<fs::File>,
}

impl InjectionLock {
    fn release(&mut self) {
        if let Some(file) = self.file.take() {
            let _ = file.unlock();
        }
        if let Some(path) = self.path.take() {
            let _ = fs::remove_file(path);
        }
    }
}

impl Drop for InjectionLock {
    fn drop(&mut self) {
        self.release();
    }
}

fn record_run(footprint: &Footprint) -> std::io::Result<Option<PathBuf>> {
    if footprint.files.is_empty() && footprint.dirs.is_empty() {
        if run_is_announced() {
            finish_run();
        }
        return Ok(None);
    }
    let dir = seam_record_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the workspace root could not be resolved, so this run cannot record what it did",
        )
    })?;
    fs::create_dir_all(&dir)?;
    if !run_is_announced() {
        take_run_lock(&dir)?;
    }
    let (path, _) = run_record_and_lock(&dir);
    write_record(&path, footprint)?;
    Ok(Some(path))
}

fn finish_run() {
    LIVE_RUN.with(|cell| {
        if let Some(run) = cell.borrow_mut().take() {
            let _ = fs::remove_file(&run.path);
        }
    });
    release_injection_locks();
}

fn begin_run() -> std::io::Result<()> {
    if run_is_announced() || !writes_a_covered_crate() {
        return Ok(());
    }
    let Some(dir) = seam_record_dir() else {
        return Ok(());
    };
    fs::create_dir_all(&dir)?;
    take_run_lock(&dir)?;
    let (path, _) = run_record_and_lock(&dir);
    write_record(&path, &Footprint::default())?;
    LIVE_RUN.with(|cell| {
        *cell.borrow_mut() = Some(LiveRun { path });
    });
    Ok(())
}

fn writes_a_covered_crate() -> bool {
    covered_crate_root().is_some()
}

fn covered_crate_root() -> Option<PathBuf> {
    let mut roots: Vec<String> = std::env::args().skip(1).collect();
    roots.reverse();
    let root = roots.into_iter().find(|arg| arg.ends_with(".rs") && !arg.starts_with('-'))?;
    let source = PathBuf::from(root);
    let source = match fs::canonicalize(&source) {
        Ok(abs) => strip_unc(abs),
        Err(_) => source,
    };
    let crate_src = source.parent()?.to_path_buf();
    let subject = crate_src.to_string_lossy().replace('\\', "/");
    match rules::cover(&subject) {
        rules::Coverage::Claimed(_) => Some(crate_src),
        _ => None,
    }
}

fn run_is_announced() -> bool {
    LIVE_RUN.with(|cell| cell.borrow().is_some())
}

fn write_record(path: &Path, footprint: &Footprint) -> std::io::Result<()> {
    let Some(dir) = path.parent() else {
        return Ok(());
    };
    let saved = SavedRun {
        files: footprint
            .files
            .iter()
            .map(|entry| SavedFile {
                path: entry.path.clone(),
                original: entry.original.as_ref().map(|bytes| base64_encode(bytes)),
            })
            .collect(),
        dirs: footprint.dirs.clone(),
    };
    let bytes = serde_json::to_string(&saved)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let staging = dir.join(format!("{}{SEAM_RUN_STAGING_SUFFIX}", std::process::id()));
    fs::write(&staging, bytes)?;
    fs::rename(&staging, path)?;
    Ok(())
}

struct LiveRun {
    path: PathBuf,
}

thread_local! {
    static LIVE_RUN: std::cell::RefCell<Option<LiveRun>> = const { std::cell::RefCell::new(None) };
    static LIVE_LOCKS: std::cell::RefCell<Vec<InjectionLock>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn refresh_live_record(footprint: &Footprint) -> std::io::Result<()> {
    LIVE_RUN.with(|cell| {
        if let Some(run) = cell.borrow().as_ref() {
            write_record(&run.path, footprint)?;
        }
        Ok(())
    })
}

fn seam_record_dir() -> Option<PathBuf> {
    // the record stands beside the binary that wrote it, never under the cargo target directory of
    // the moment: a build that moves its target keeps writing the same tree, and a record left
    // where a later run does not look is a write nothing can take back
    let at = std::env::current_exe().ok()?;
    at.parent().map(|dir| dir.join(SEAM_RUN_RECORD_DIR))
}

fn record_name() -> String {
    format!("{}.json", std::process::id())
}

pub const SEAM_RUN_RECORD_DIR: &str = "lvpa-seam-run";

const SEAM_RUN_STAGING_SUFFIX: &str = ".staging";

const SEAM_RUN_LOCK_SUFFIX: &str = ".lock";

const SETTLE_READINGS: u32 = 4096;

const SETTLE_ABSENCE: u32 = 2;

const SEAM_INJECTION_LOCK: &str = ".lvpa-seam-injection.lock";

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(triple >> 18) as usize & 0x3f] as char);
        out.push(ALPHABET[(triple >> 12) as usize & 0x3f] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(triple >> 6) as usize & 0x3f] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[triple as usize & 0x3f] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        } as u32;
        acc = (acc << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn outstanding_records() -> Vec<PathBuf> {
    let mut records = Vec::new();
    let Some(dir) = seam_record_dir() else {
        return records;
    };
    let Ok(entries) = fs::read_dir(&dir) else {
        return records;
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    found.sort();
    records.extend(found);
    records
}

fn undo_recorded_run(record: &Path) -> std::io::Result<()> {
    let Ok(text) = fs::read_to_string(record) else {
        return Ok(());
    };
    let Ok(saved) = serde_json::from_str::<SavedRun>(&text) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "the interrupted run's record {} could not be read",
                record.display()
            ),
        ));
    };
    let mut touched = 0usize;
    for entry in &saved.files {
        if !entry.path.is_file() {
            continue;
        }
        match &entry.original {
            Some(bytes) => {
                let Some(bytes) = base64_decode(bytes) else {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "the interrupted run's record holds bytes this build cannot decode",
                    ));
                };
                fs::write(&entry.path, bytes)?;
            }
            None => fs::remove_file(&entry.path)?,
        }
        touched += 1;
    }
    for dir in saved.dirs.iter().rev() {
        let _ = fs::remove_dir(dir);
    }
    retire_record(record);
    if touched > 0 {
        eprintln!(
            "[lvpa-wrapper] seam healed: an interrupted run left {touched} file(s) and {} \
             directory/directories in the covered tree; they were put back from that run's own \
             record before the rules ran",
            saved.dirs.len()
        );
    }
    Ok(())
}

