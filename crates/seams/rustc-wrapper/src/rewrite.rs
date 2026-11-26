use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

use crate::rules::{self, Locate, Rule};

use std::env;

pub struct Target {
    rewritten: String,
    crlf: bool,
}

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
                 for none of them — the build would silently keep upstream behaviour",
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
                 precondition was unmet — the crate would ship with a partially active seam. \
                 Abandoned: {}",
                self.rewrote.len(),
                self.abandoned.len(),
                self.abandoned.join(", ")
            ));
        }
        None
    }
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
            if env::var("LVPA_WRAPPER_TRACE").is_ok() {
                eprintln!("[lvpa-wrapper] trace: trees_state refused: {reason}");
            }
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
    if env::var("LVPA_WRAPPER_TRACE").is_ok() {
        eprintln!("[lvpa-wrapper] trace: {} .rs argument(s) on the command line: {roots:?}", roots.len());
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
    let spec = match rules::cover(&subject) {
        rules::Coverage::Claimed(spec) => spec,
        rules::Coverage::Uncovered => {
            if env::var("LVPA_WRAPPER_TRACE").is_ok() {
                eprintln!("[lvpa-wrapper] trace: uncovered {subject}");
            }
            return report;
        }
        rules::Coverage::Absorbed => {
            if env::var("LVPA_WRAPPER_TRACE").is_ok() {
                eprintln!("[lvpa-wrapper] trace: absorbed {subject}");
            }
            return report;
        }
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
        if !source.is_file() {
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
    let root = strip_unc(root);

    let injected = match inject(&root, &crate_src, spec) {
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
                report.misses.push(describe_miss(spec, &id));
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
    root: &Path,
    crate_src: &Path,
    spec: &'static rules::CrateSpec,
) -> std::io::Result<InjectResult> {
    let injection_lock = take_injection_lock(crate_src)?;
    let seams = spec.seams;
    let mut applied = Vec::new();
    let mut missed = Vec::new();
    let mut abandoned = Vec::new();
    let mut by_segment: BTreeMap<Vec<String>, Target> = BTreeMap::new();
    let mut footprint = Footprint::default();
    let root_segments: Vec<String> = root
        .strip_prefix(crate_src)
        .map(|rel| {
            rel.components()
                .map(|component| component.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
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
    for seam in seams {
        let Some(host) = workspace_root_path().map(|root| root.join(seam.host.as_str_checked()))
        else {
            missed.push(format!("module_decl.{}", seam.module));
            continue;
        };
        match write_seam_module(&host, crate_src, seam, &mut footprint) {
            Ok(Some(declaration)) => declared_modules.push(declaration),
            Ok(None) => {}
            Err(error) => abandoned.push(format!("module_decl.{}: {error}", seam.module)),
        }
        seam_dependencies.push(seam.dependencies);
    }
    let mut declarations: Vec<&rules::SeamDependency> = seam_dependencies.iter().copied().flatten().collect();
    declarations.extend(
        spec.dependencies
            .iter()
            .filter(|dependency| matches!(dependency.landing, rules::Landing::HostManifest)),
    );
    if !declarations.is_empty() {
        let manifest = crate_src
            .ancestors()
            .find(|dir| dir.join("Cargo.toml").is_file())
            .map(|dir| dir.join("Cargo.toml"));
        match manifest {
            Some(manifest) => match declare_dependencies(&manifest, declarations.as_slice(), &mut footprint) {
                Ok(()) => {}
                Err(error) => abandoned.push(format!("dependency: {error}")),
            },
            None => abandoned.push("dependency: no manifest above the crate".to_string()),
        }
    }
    for dependency in spec
        .dependencies
        .iter()
        .filter(|dependency| matches!(dependency.landing, rules::Landing::SeamModule))
    {
        if let Err(error) = replace_an_edge(&host_manifest, dependency, &mut footprint) {
            unreachable_landing(crate_src, None, &format!("dependency.{}: {error}", dependency.package));
        }
        if dependency.managed.adds_an_edge() {
            let body = match workspace_root_path().map(|root| root.join(dependency.path).join("Cargo.toml")) {
                Some(manifest) if manifest.is_file() => manifest,
                _ => {
                    missed.push(format!("dependency.{}.Cargo.toml", dependency.package));
                    continue;
                }
            };
            let one: [&rules::SeamDependency; 1] = [dependency];
            let landed = declare_dependencies(&body, &one, &mut footprint);
            match landed {
                Ok(()) => {}
                Err(error) => missed.push(format!("dependency.{}: {error}", dependency.package)),
            }
        }
    }
    for (module, seeds) in &declared_modules {
        for seed_at in *seeds {
            let anchor = crate_src.join(seed_at);
            match declare_module(&anchor, module, &mut footprint) {
                Ok(()) => {}
                Err(error) => abandoned.push(format!("module_decl.{module}: {error}")),
            }
        }
    }
    let mut extern_crates: Vec<String> = Vec::new();
    for dependency in seam_dependencies.iter().copied().flatten() {
        if let Some(name) = dependency.extern_crate() {
            if !extern_crates.contains(&name) {
                extern_crates.push(name);
            }
        }
    }
    for path in seam_module_paths(crate_src, &declared_modules) {
        let mut module_text = normalize(&fs::read_to_string(&path)?);
        for source in seam_dependencies.iter().copied().flatten() {
            let declared = source.declared_name();
            if source.name != declared {
                module_text = module_text.replace(
                    &format!("use {}::", source.name.replace('-', "_")),
                    &format!("use {declared}::"),
                );
            }
        }
        let mut landed = declare_extern_crates(&module_text, &extern_crates);
        landed = drop_legacy_bindings(&landed, &extern_crates);
        write_in_place(&path, landed.into_bytes(), &mut footprint)?;
    }
    for rule in spec.rules {
        let target = crate_src.join(rule.file);
        if !target.is_file() {
            unreachable_landing(
                crate_src,
                Some(rule),
                &format!("{} is not in the crate", rule.file),
            );
        }
        if !seam_is_landed(crate_src, rule) {
            unreachable_landing(crate_src, Some(rule), "its seam module did not land in this crate");
        }

        let Ok(rel) = target.strip_prefix(crate_src) else {
            unreachable_landing(crate_src, Some(rule), &format!("{} is outside the crate", rule.file));
        };
        let segments: Vec<String> = rel
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect();
        if segments.is_empty() {
            unreachable_landing(crate_src, Some(rule), "it lands on no source of the crate");
        }

        let previous = by_segment.get(&segments).map(|target| target.rewritten.clone());
        let source = match previous.clone() {
            Some(text) => text,
            None => fs::read_to_string(&target)?,
        };
        let rewritten = match rewrite_text(&target, rule, previous.as_deref())? {
            Resolution::Rewritten(text) => text,
            Resolution::NotApplicable => {
                missed.push(rule.id.to_string());
                continue;
            }
            Resolution::AnchorMissing => unreachable_landing(
                crate_src,
                Some(rule),
                "the seam is here and the anchor it is written for is gone",
            ),
        };
        by_segment.insert(
            segments,
            Target {
                rewritten,
                crlf: source.contains("\r\n"),
            },
        );
        applied.push(rule.id.to_string());
    }

    for segments in seam_write_order(&root_segments, by_segment.keys()) {
        if let Some(target) = by_segment.get(&segments) {
            let file = crate_src.join(segments.join("/"));
            let text = if target.crlf {
                target.rewritten.replace('\n', "\r\n")
            } else {
                target.rewritten.clone()
            };
            write_in_place(&file, text.into_bytes(), &mut footprint)?;
        }
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

fn replace_an_edge(
    manifest: &Path,
    dependency: &rules::SeamDependency,
    footprint: &mut Footprint,
) -> Result<(), String> {
    let text = normalize(
        &fs::read_to_string(manifest)
            .map_err(|error| format!("{} is not readable: {error}", manifest.display()))?,
    );
    let target = normalized_package(dependency.package);
    let reached = |line: &str| {
        edge_package(line).is_some_and(|held| normalized_package(&held) == target)
    };
    let Some(standing) = text
        .split_inclusive('\n')
        .enumerate()
        .find(|(_, line)| reached(line) && edge_key(line).is_some_and(|key| key == dependency.declared_name()))
        .map(|(index, _)| index)
    else {
        return Err(format!(
            "{} carries no `{}` edge, and a seam replaces that edge rather than standing beside it",
            manifest.display(),
            dependency.declared_name()
        ));
    };
    let landed = dependency.origin(&manifest_spec_path(
        &workspace_root_path().unwrap_or_else(|| PathBuf::from(".")),
        manifest.parent().unwrap_or(manifest),
    ));
    let mut replaced: Vec<String> = Vec::new();
    for (index, line) in text.split_inclusive('\n').enumerate() {
        if index != standing && reached(line) {
            continue;
        }
        if index != standing {
            replaced.push(line.to_string());
            continue;
        }
        let Some(cut) = line.find('=') else {
            replaced.push(line.to_string());
            continue;
        };
        let alias = line[..cut].trim_end();
        let ending = if line.ends_with('\n') { "\n" } else { "" };
        replaced.push(format!(
            "{alias} = {{ package = \"{}\", {landed}}}{ending}",
            dependency.package
        ));
    }
    write_in_place(manifest, replaced.concat().into_bytes(), footprint)
        .map_err(|error| error.to_string())?;
    Ok(())
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

fn seam_write_order<'a>(
    root_segments: &[String],
    keys: impl Iterator<Item = &'a Vec<String>>,
) -> Vec<Vec<String>> {
    let mut order: Vec<Vec<String>> = Vec::new();
    let mut rest: Vec<Vec<String>> = Vec::new();
    for key in keys {
        if key.as_slice() == root_segments {
            order.push(key.clone());
        } else {
            rest.push(key.clone());
        }
    }
    order.extend(rest);
    order
}

fn write_seam_module(
    src: &Path,
    crate_src: &Path,
    seam: &rules::Seam,
    footprint: &mut Footprint,
) -> std::io::Result<Option<(&'static str, &'static [&'static str])>> {
    let Some(first) = seam.seeds.first() else {
        return Err(landing_miss(
            src,
            seam.module,
            "the seam names no crate root, so its module has no directory to land in",
        ));
    };
    let module_dir = match crate_src.join(first).parent() {
        Some(dir) => dir.join(seam.module),
        None => crate_src.join(seam.module),
    };
    let module_file = module_dir.join("mod.rs");
    let body = fs::read_to_string(src)?;
    let body = land_seam_source(&body, src, seam.module)?;
    if let Some(dir) = src.parent() {
        for entry in fs::read_dir(dir)?.flatten() {
            let path = entry.path();
            if !path.is_file() || path == src {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            let landed = module_dir.join(name);
            if path.extension().is_some_and(|ext| ext == "rs") {
                let sibling = fs::read_to_string(&path)?;
                let body = land_seam_source(&sibling, &path, seam.module)?;
                write_in_place(&landed, body.into_bytes(), footprint)?;
            } else {
                let bytes = fs::read(&path)?;
                write_in_place(&landed, bytes, footprint)?;
            }
        }
    }
    write_in_place(&module_file, body.into_bytes(), footprint)?;
    Ok(Some((seam.module, seam.seeds)))
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

fn declare_module(
    anchor: &Path,
    module: &str,
    footprint: &mut Footprint,
) -> std::io::Result<()> {
    let text = normalize(&fs::read_to_string(anchor)?);
    if modules_of(&text).iter().any(|name| name == module) {
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

fn declaration_after_inner_attributes(text: &str) -> usize {
    let mut at = 0usize;
    while at < text.len() {
        let end = match text[at..].find('\n') {
            Some(offset) => at + offset + 1,
            None => text.len(),
        };
        let body = text[at..end].trim();
        if body.starts_with("//!") || body.starts_with("#![") {
            at = end;
            continue;
        }
        break;
    }
    at
}

fn declare_extern_crates(text: &str, crates: &[String]) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut cut = lines
        .iter()
        .position(|line| !line.starts_with("//!") && !is_module_declaration(line))
        .unwrap_or(lines.len());
    while cut > 0 && lines[cut - 1].trim().is_empty() {
        cut -= 1;
    }
    let pending: Vec<String> = crates
        .iter()
        .filter(|name| !text.contains(&format!("extern crate {name};")))
        .map(|name| format!("extern crate {name};\n"))
        .collect();
    let mut head: String = lines[..cut].iter().map(|line| format!("{line}\n")).collect();
    for binding in pending {
        head.push_str(&binding);
    }
    let tail: String = lines[cut..].iter().map(|line| format!("{line}\n")).collect();
    format!("{head}{tail}")
}

fn drop_legacy_bindings(text: &str, keep: &[String]) -> String {
    text.lines()
        .filter(|line| match line.trim().strip_prefix("extern crate ") {
            Some(rest) => {
                let name = rest.trim_end_matches(';').trim();
                keep.iter().any(|kept| kept == name)
            }
            None => true,
        })
        .map(|line| format!("{line}\n"))
        .collect()
}

fn is_module_declaration(line: &str) -> bool {
    let trimmed = line.trim_start();
    let body = trimmed.strip_prefix("pub ").unwrap_or(trimmed);
    body.starts_with("mod ") || body.starts_with("pub mod ")
}

fn declare_dependencies(
    manifest: &Path,
    dependencies: &[&rules::SeamDependency],
    footprint: &mut Footprint,
) -> std::io::Result<()> {
    let text = normalize(&fs::read_to_string(manifest)?);
    let header = "[dependencies]";
    let mut lines: Vec<String> = text.split_inclusive('\n').map(|line| line.to_string()).collect();
    let mut declared = String::with_capacity(dependencies.len() * 64);
    for dependency in dependencies.iter().copied() {
        let existing = declared.clone();
        let target = normalized_package(dependency.package);
        lines.retain(|line| {
            if edge_package(line).is_some_and(|reached| normalized_package(&reached) == target) {
                return false;
            }
            !declares_a_seam_beside(line, dependency.path)
        });
        if !declares_a_package(&existing, &target) && !declares_a_key(&existing, &dependency.declared_name()) {
            declared.push_str(&inline_dependency(dependency, manifest));
        }
    }
    if declared.is_empty() {
        return Ok(());
    }
    let at = lines.iter().position(|line| line.trim_end() == header);
    match at {
        Some(at) => {
            let mut end = at + 1;
            while end < lines.len() && !lines[end].trim_start().starts_with('[') {
                end += 1;
            }
            if !lines[end - 1].ends_with('\n') {
                lines[end - 1].push('\n');
            }
            lines.insert(end, declared);
        }
        None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "{header} is absent from {}, so the seam dependency has no section to land in",
                    manifest.display()
                ),
            ));
        }
    }
    let landed = lines.concat();
    write_in_place(manifest, landed.into_bytes(), footprint)?;
    Ok(())
}

fn declares_a_seam_beside(line: &str, path: &str) -> bool {
    line.contains(&format!("path = \"{path}\""))
}

fn declares_a_package(text: &str, target: &str) -> bool {
    text.lines()
        .any(|line| edge_package(line).is_some_and(|reached| normalized_package(&reached) == target))
}

fn declares_a_key(text: &str, key: &str) -> bool {
    text.lines().any(|line| edge_key(line).is_some_and(|held| held == key))
}

fn inline_dependency(dependency: &rules::SeamDependency, manifest: &Path) -> String {
    let derived = workspace_root_path().map(|root| root.join(dependency.path));
    let landed = derived.as_ref().filter(|path| path.is_file());
    let crate_dir = match landed {
        Some(file) => file.parent().map(Path::to_path_buf).unwrap_or_else(|| file.clone()),
        None => match derived {
            Some(path) => path,
            None => match manifest.parent() {
                Some(dir) => dir.join(dependency.path),
                None => PathBuf::from(dependency.path),
            },
        },
    };
    let relative = if dependency.path.starts_with('.') {
        dependency.path.to_string()
    } else if dependency.declared.is_empty() && !dependency.package.starts_with("lvpa-") {
        relative_manifest_path(crate_dir.as_path(), manifest)
    } else {
        match workspace_root_path() {
            Some(root) => manifest_spec_path(root.as_path(), crate_dir.as_path()),
            None => relative_manifest_path(crate_dir.as_path(), manifest),
        }
    };
    dependency.line(&relative)
}

fn manifest_spec_path(root: &Path, crate_dir: &Path) -> String {
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
    let crlf = text.contains("\r\n");
    let lf = text.replace("\r\n", "\n");
    if syn::parse_file(&lf).is_err() {
        return Err(landing_miss(
            source,
            module,
            "the seam source does not parse, so it cannot be read back as the module body",
        ));
    }
    let landed = reanchor_seam_crate_paths(&lf, module);
    Ok(if crlf {
        landed.replace('\n', "\r\n")
    } else {
        landed
    })
}

fn reanchor_seam_crate_paths(text: &str, module: &str) -> String {
    let mut out = String::with_capacity(text.len() + 64);
    let mut rest = text;
    while let Some(index) = rest.find("crate::") {
        out.push_str(&rest[..index]);
        rest = &rest[index + "crate::".len()..];
        let own = format!("{module}::");
        if rest.starts_with(&own) {
            rest = &rest[own.len()..];
            out.push_str("crate::");
        } else {
            out.push_str(&format!("crate::{module}::"));
        }
    }
    out.push_str(rest);
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

fn save_before_write(path: &Path, footprint: &mut Footprint) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
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
    let mut missing: Vec<&Path> = Vec::new();
    let mut cursor = Some(dir);
    while let Some(path) = cursor {
        if path.is_dir() {
            break;
        }
        missing.push(path);
        cursor = path.parent();
    }
    for path in missing.iter().rev() {
        fs::create_dir(path)?;
        footprint.dirs.push((*path).to_path_buf());
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
                site,
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
            match crate::rules::composition::wrap(&current, &carried, envelope, entry, site) {
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
    }
}

fn seam_is_landed(crate_src: &Path, rule: &Rule) -> bool {
    let Some(module) = rule_seam_module(rule) else {
        return true;
    };
    let seam = rules::registered()
        .into_iter()
        .flat_map(|spec| spec.seams.iter())
        .find(|seam| seam.module == module);
    let Some(seam) = seam else {
        return false;
    };
    seam.seeds.iter().all(|seed_at| {
        let decl = crate_src.join(seed_at);
        let seed = match decl.parent() {
            Some(dir) => dir.join(module).join("mod.rs"),
            None => return false,
        };
        seed.is_file()
            && fs::read_to_string(&decl)
                .map(|text| modules_of(&text).iter().any(|name| name == module))
                .unwrap_or(false)
    })
}

pub fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
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
    if let Some(target) = std::env::var_os(CARGO_TARGET_DIR_ENV) {
        let target = PathBuf::from(target);
        if !target.as_os_str().is_empty() {
            return Some(target.join(SEAM_RUN_RECORD_DIR));
        }
    }
    match workspace_root_path() {
        Some(root) => Some(root.join("target").join(SEAM_RUN_RECORD_DIR)),
        None => {
            eprintln!(
                "[lvpa-wrapper] FATAL: the workspace root could not be resolved and no {CARGO_TARGET_DIR_ENV} names \
                 where this run records what it writes, so a write it could not take back would stand. Nothing is \
                 rewritten."
            );
            exit(97);
        }
    }
}

const CARGO_TARGET_DIR_ENV: &str = "CARGO_TARGET_DIR";

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

pub fn modules_of(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(file) = syn::parse_file(text) else {
        return out;
    };
    for item in &file.items {
        if let syn::Item::Mod(module) = item {
            if module.content.is_none() {
                out.push(module.ident.to_string());
            }
        }
    }
    out
}
