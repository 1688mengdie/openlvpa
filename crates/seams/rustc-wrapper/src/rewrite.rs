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
    locks: Vec<InjectionLock>,
}

impl Report {
    pub fn any(&self) -> bool {
        !self.rewrote.is_empty()
    }

    pub fn self_check(&self, covered: usize, taken: usize) -> Option<String> {
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
        if self.any() && covered != taken {
            return Some(format!(
                "{} rule(s) reported as applied ({}), but a rewritten source reached the compiler \
                 for only {taken} of {covered} covered crate(s) — the build would silently keep \
                 upstream behaviour for the rest",
                self.rewrote.len(),
                self.rewrote.join(", ")
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
        locks: Vec::new(),
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

    for (declaration, detail) in rules::unresolved_declarations() {
        eprintln!(
            "[lvpa-wrapper] FATAL: the rule table's declaration `{declaration}` did not resolve, so \
             {detail}. Nothing is rewritten against an unresolved table."
        );
        exit(97);
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

    let mut roots = Vec::new();
    for arg in args.iter() {
        if !arg.ends_with(".rs") || arg.starts_with('-') {
            continue;
        }
        let root = PathBuf::from(arg);
        let root = match fs::canonicalize(&root) {
            Ok(abs) => strip_unc(abs),
            Err(_) => root,
        };
        roots.push(root);
    }
    if env::var("LVPA_WRAPPER_TRACE").is_ok() {
        eprintln!("[lvpa-wrapper] trace: {} .rs argument(s) on the command line: {roots:?}", roots.len());
    }
    let Some(root) = roots.pop() else {
        return report;
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
        rules::Coverage::Orphan(spec) => {
            eprintln!(
                "[lvpa-wrapper] FATAL: {subject} sits inside the tree declared as {}, yet no crate \
                 declaration claims it. A crate the declaration does not name would be built \
                 without the seam it owes, so the pass stops here.",
                spec.source_tree
            );
            exit(93);
        }
    };

    let present_unit = unit_of(&crate_src, args);
    let mut covered = 0usize;
    let mut taken = 0usize;
    covered += 1;

    let mut missing_seams: Vec<(&str, String)> = Vec::new();
    for seam in spec.seams {
        let source = workspace_root_path().map(|root| root.join(seam.host.as_str_checked()));
        let readable = source.as_ref().map(|path| path.is_file()).unwrap_or(false);
        if !readable {
            let reason = match &source {
                Some(path) => format!("{} is not a readable file", path.display()),
                None => "the workspace root could not be resolved".to_string(),
            };
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
        if let Some(reason) = report.self_check(covered, taken) {
            eprintln!("[lvpa-wrapper] FATAL: seam self-check failed: {reason}");
            restore_then_exit(&report, 93);
        }
        return report;
    }

    if !root.is_file() {
        eprintln!(
            "[lvpa-wrapper] FATAL: the covered crate root {subject} declares {package} but the \
             compiler was handed no file of it, so the seam has nowhere to land",
            package = spec.package
        );
        restore_then_exit(&report, 93);
    }

    match inject(&root, &crate_src, present_unit, spec) {
        Ok(injected) => {
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
            report.locks.push(injected.lock);
            taken += 1;
        }
        Err(error) => {
            eprintln!(
                "[lvpa-wrapper] FATAL: could not inject the seam for {}: {error}",
                crate_src.display()
            );
            report.abandoned.extend(
                spec.rules
                    .iter()
                    .map(|rule| rule.reported_id(spec.source_tree)),
            );
        }
    }

    if let Some(reason) = report.self_check(covered, taken) {
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
    present_unit: rules::RuleUnit,
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

    for seam in seams {
        let Some(host) = workspace_root_path().map(|root| root.join(seam.host.as_str_checked()))
        else {
            missed.push(format!("module_decl.{}", seam.module));
            continue;
        };
        if let Err(error) = write_seam_module(&host, crate_src, seam, &mut footprint) {
            abandoned.push(format!("module_decl.{}: {error}", seam.module));
        }
    }

    for rule in spec.rules {
        if !rule_unit(rule).runs_in(present_unit) {
            missed.push(rule.id.to_string());
            continue;
        }
        let target = crate_src.join(rule.file);
        if !target.is_file() {
            missed.push(rule.id.to_string());
            continue;
        }
        let Ok(rel) = target.strip_prefix(crate_src) else {
            missed.push(rule.id.to_string());
            continue;
        };
        let segments: Vec<String> = rel
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect();
        if segments.is_empty() {
            missed.push(rule.id.to_string());
            continue;
        }

        let previous = by_segment.get(&segments).map(|target| target.rewritten.clone());
        let Some(rewritten) = rewrite_text(&target, rule, previous.as_deref())? else {
            missed.push(rule.id.to_string());
            continue;
        };

        let source = fs::read_to_string(&target)?;
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
) -> std::io::Result<()> {
    let module_dir = crate_src.join(seam.module);
    let module_file = module_dir.join("mod.rs");
    let body = fs::read_to_string(src)?;
    let body = land_seam_source(&body, src, seam.module)?;
    write_in_place(&module_file, body.into_bytes(), footprint)?;
    let Some(src_dir) = src.parent() else {
        return Ok(());
    };
    for entry in fs::read_dir(src_dir)?.flatten() {
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
    Ok(())
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
    let prefix = format!("crate::{module}::");
    let mut out = String::with_capacity(text.len() + 64);
    let mut rest = text;
    while let Some(index) = rest.find("crate::") {
        out.push_str(&rest[..index]);
        let after = &rest[index + "crate::".len()..];
        let skip = format!("{module}::");
        if after.starts_with(&skip) {
            rest = &after[skip.len()..];
        } else {
            rest = after;
        }
        out.push_str(&prefix);
    }
    out.push_str(rest);
    out
}

fn write_in_place(path: &Path, bytes: Vec<u8>, footprint: &mut Footprint) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        create_dir_recording(dir, footprint)?;
    }
    if !footprint.files.iter().any(|saved| saved.path == path) {
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
    }
    refresh_live_record(footprint)?;
    fs::write(path, bytes)
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

fn unit_of(crate_src: &Path, args: &[String]) -> rules::RuleUnit {
    let is_src = crate_src
        .file_name()
        .map(|name| name == "src")
        .unwrap_or(false);
    if is_src {
        return if is_test_build(args) {
            rules::RuleUnit::UnitTest
        } else {
            rules::RuleUnit::Library
        };
    }
    if crate_src
        .file_name()
        .map(|name| name == "tests")
        .unwrap_or(false)
    {
        return rules::RuleUnit::TestTarget;
    }
    rules::RuleUnit::NonLibrary
}

fn is_test_build(args: &[String]) -> bool {
    let is_test_cfg = |value: &str| value == "test" || value.trim_end_matches(')').ends_with(",test");
    for (index, arg) in args.iter().enumerate() {
        if arg == "--test" {
            return true;
        }
        if let Some(body) = arg.strip_prefix("--cfg=") {
            if is_test_cfg(body) {
                return true;
            }
        } else if arg == "--cfg" {
            if let Some(next) = args.get(index + 1) {
                if is_test_cfg(next) {
                    return true;
                }
            }
        }
    }
    false
}

pub fn rewrite_text(
    path: &Path,
    rule: &Rule,
    previous: Option<&str>,
) -> std::io::Result<Option<String>> {
    let applied = apply_rule(path, rule, previous)?;
    Ok(applied.applied.then_some(applied.text))
}

pub struct Applied {
    pub text: String,
    pub applied: bool,
}

pub fn apply_rule(path: &Path, rule: &Rule, previous: Option<&str>) -> std::io::Result<Applied> {
    let source = match previous {
        Some(text) => text.to_string(),
        None => fs::read_to_string(path)?,
    };
    let crlf = source.contains("\r\n");
    let current = normalize(&source);

    let resolved = match &rule.locate {
        Locate::Composition(envelope) => match rule_seam(rule) {
            Some(seam) => match crate::rules::composition::commands(&current) {
                Some(commands) => {
                    let table = crate::rules::composition::embed(
                        seam.host.as_str_checked(),
                        &commands,
                    );
                    match table {
                        Some(body) => {
                            let landed = write_seam_table(seam, &body)?;
                            let landed = landed.to_string_lossy().into_owned();
                            crate::rules::composition::wrap(&current, &landed, envelope)
                                .map(|composition| composition.text)
                        }
                        None => None,
                    }
                }
                None => None,
            },
            None => None,
        },
    };

    let Some(rewritten) = resolved else {
        return Ok(Applied {
            text: source,
            applied: false,
        });
    };

    Ok(Applied {
        text: if crlf {
            rewritten.replace('\n', "\r\n")
        } else {
            rewritten
        },
        applied: true,
    })
}

fn rule_seam(rule: &Rule) -> Option<&'static rules::Seam> {
    match &rule.locate {
        Locate::Composition(envelope) => rules::registered()
            .into_iter()
            .flat_map(|spec| spec.seams.iter())
            .find(|seam| seam.host.as_str_checked() == envelope.host),
    }
}

fn write_seam_table(seam: &rules::Seam, body: &str) -> std::io::Result<PathBuf> {
    let Some(root) = workspace_root_path() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the workspace root could not be resolved, so the seam table has nowhere to land",
        ));
    };
    let module_root = root.join(seam.body.as_str_checked());
    let module_dir = match module_root.parent() {
        Some(dir) => dir,
        None => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "the seam body has no directory to land its table beside",
            ))
        }
    };
    let landed = module_dir.join("table.rs");
    fs::write(&landed, body)?;
    Ok(landed)
}

fn rule_unit(rule: &Rule) -> rules::RuleUnit {
    match rule.locate {
        Locate::Composition(_) => rules::RuleUnit::Any,
    }
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
    let Some(root) = seam_record_dir() else {
        return Ok(InjectionLock { file: None });
    };
    fs::create_dir_all(&root)?;
    let path = injection_lock_path(crate_src).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the record directory disappeared between being created and the lock being placed",
        )
    })?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    file.lock()?;
    Ok(InjectionLock { file: Some(file) })
}

fn injection_lock_path(crate_src: &Path) -> Option<PathBuf> {
    seam_record_dir().map(|root| root.join(format!("{SEAM_INJECTION_LOCK}{}", crate_key(crate_src))))
}

fn crate_key(crate_src: &Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    crate_src.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

struct InjectionLock {
    file: Option<fs::File>,
}

impl Drop for InjectionLock {
    fn drop(&mut self) {
        if let Some(file) = &self.file {
            let _ = file.unlock();
        }
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
}

fn begin_run() -> std::io::Result<()> {
    if run_is_announced() {
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
    if let Some(target) = std::env::var_os("CARGO_TARGET_DIR") {
        let target = PathBuf::from(target);
        if !target.as_os_str().is_empty() {
            return Some(target.join(SEAM_RUN_RECORD_DIR));
        }
    }
    workspace_root_path().map(|root| root.join(".openlvpa").join(SEAM_RUN_RECORD_DIR))
}

fn seam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(root) = workspace_root_path() {
        roots.push(root);
    }
    if let Ok(compilation) = env::var(CARGO_TARGET_DIR_ENV) {
        let dir = PathBuf::from(&compilation);
        let dir = if dir.is_absolute() {
            dir
        } else {
            env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(dir)
        };
        if let Ok(canonical) = fs::canonicalize(&dir) {
            roots.push(strip_unc(canonical));
        }
    }
    roots
}

const CARGO_TARGET_DIR_ENV: &str = "CARGO_TARGET_DIR";

fn record_name() -> String {
    format!("{}.json", std::process::id())
}

const SEAM_RUN_RECORD: &str = "lvpa-seam-run.json";

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
    for root in seam_roots() {
        let legacy = root.join("target").join(SEAM_RUN_RECORD);
        if legacy.is_file() {
            records.push(legacy);
        }
        let Ok(entries) = fs::read_dir(root.join("target").join(SEAM_RUN_RECORD_DIR)) else {
            continue;
        };
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        found.sort();
        records.extend(found);
    }
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
