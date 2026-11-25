use std::path::Path;
use std::process::exit;

pub mod composition;
mod openalgo_backend;

pub const DECLARATION: &str = "scripts/absorbed-sources.json";

pub struct CrateSpec {
    pub crate_path: &'static str,
    pub source_tree: &'static str,
    pub package: &'static str,
    pub crate_name: &'static str,
    pub package_module: Option<&'static str>,
    pub seams: &'static [Seam],
    pub rules: &'static [Rule],
}

pub struct SeamHost(&'static str);

pub struct SeamBody(&'static str);

impl SeamHost {
    pub const fn checked(path: &'static str) -> Self {
        assert!(
            is_workspace_relative_rust_source(path),
            "a seam host must be a workspace-root-relative .rs path"
        );
        Self(path)
    }

    pub const fn as_str_checked(&self) -> &'static str {
        self.0
    }
}

impl SeamBody {
    pub const fn checked(path: &'static str) -> Self {
        assert!(
            is_workspace_relative_rust_source(path),
            "a seam body must be a workspace-root-relative .rs path"
        );
        Self(path)
    }

    pub const fn as_str_checked(&self) -> &'static str {
        self.0
    }
}

const fn is_workspace_relative_rust_source(path: &str) -> bool {
    let bytes = path.as_bytes();
    if bytes.is_empty()
        || bytes[0] == b'/'
        || bytes[0] == b'\\'
        || (bytes.len() >= 2 && bytes[1] == b':')
    {
        return false;
    }
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'.' && bytes[index + 1] == b'.' {
            return false;
        }
        index += 1;
    }
    bytes.len() > 3
        && bytes[bytes.len() - 3] == b'.'
        && bytes[bytes.len() - 2] == b'r'
        && bytes[bytes.len() - 1] == b's'
}

pub struct Seam {
    pub module: &'static str,
    pub host: SeamHost,
    pub body: SeamBody,
}

pub struct Rule {
    pub id: &'static str,
    pub file: &'static str,
    pub locate: Locate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuleUnit {
    Any,
    Library,
    NonLibrary,
    TestTarget,
    UnitTest,
}

impl std::fmt::Display for RuleUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl RuleUnit {
    pub fn runs_in(self, present: RuleUnit) -> bool {
        match self {
            RuleUnit::Any => matches!(
                present,
                RuleUnit::Library | RuleUnit::NonLibrary | RuleUnit::UnitTest
            ),
            RuleUnit::Library => present == RuleUnit::Library,
            RuleUnit::NonLibrary => present == RuleUnit::NonLibrary,
            RuleUnit::TestTarget => present == RuleUnit::TestTarget,
            RuleUnit::UnitTest => present == RuleUnit::UnitTest,
        }
    }
}

impl Rule {
    pub fn reported_id(&self, source_tree: &str) -> String {
        format!("{source_tree}.{}", self.id)
    }
}

pub enum Locate {
    Composition(composition::Composition),
}

pub const DECLARATIONS: &[&CrateSpec] = &[&openalgo_backend::SPEC];

pub fn registered() -> Vec<&'static CrateSpec> {
    DECLARATIONS.to_vec()
}

pub fn seam_sources() -> Vec<(&'static str, &'static str)> {
    let mut out: Vec<(&'static str, &'static str)> = Vec::new();
    for spec in registered() {
        for seam in spec.seams {
            let pair = (seam.module, seam.host.as_str_checked());
            if !out.contains(&pair) {
                out.push(pair);
            }
        }
    }
    out
}

pub fn covered_crate_packages() -> Vec<&'static str> {
    registered().iter().map(|spec| spec.package).collect()
}

pub fn covered_crate_rules() -> Vec<(&'static str, String, RuleUnit)> {
    let mut out: Vec<(&'static str, String, RuleUnit)> = Vec::new();
    for spec in registered() {
        for rule in spec.rules {
            out.push((
                spec.package,
                rule.reported_id(spec.source_tree),
                rule_unit(rule),
            ));
        }
    }
    out
}

fn rule_unit(rule: &Rule) -> RuleUnit {
    match rule.locate {
        Locate::Composition(_) => RuleUnit::Any,
    }
}

pub fn unresolved_declarations() -> Vec<(&'static str, String)> {
    let error = match crate::sources::trees() {
        Ok(_) => return Vec::new(),
        Err(error) => error.to_string(),
    };
    let mut out: Vec<(&'static str, String)> = Vec::new();
    for spec in registered() {
        if out.iter().any(|(tree, _)| *tree == spec.source_tree) {
            continue;
        }
        out.push((spec.source_tree, error.clone()));
    }
    out
}

pub fn trees_state() -> Result<(), String> {
    let (repository, trees) = match crate::sources::trees() {
        Ok(read) => read,
        Err(error) => return Err(error.to_string()),
    };
    let absent: Vec<String> = trees
        .iter()
        .filter(|tree| !crate::sources::resolve_root(tree, &repository).is_dir())
        .map(|tree| tree.root.clone())
        .collect();
    if absent.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} declares source tree(s) this checkout does not hold: {}. The take step has not run \
         against this checkout, so no rule can anchor anything.",
        DECLARATION,
        absent.join(", ")
    ))
}

fn declared_crate_root(spec: &CrateSpec) -> Option<String> {
    let (repository, trees) = crate::sources::trees().ok()?;
    let tree = trees.iter().find(|tree| tree.id == spec.source_tree)?;
    let root = crate::sources::resolve_root(tree, &repository);
    let src = root.join(spec.crate_path);
    if !holds_a_crate_root(&src) {
        return None;
    }
    Some(crate::sources::normalize(&src))
}

fn declared_subdirectory(spec: &CrateSpec, subject: &str) -> bool {
    let (repository, trees) = match crate::sources::trees() {
        Ok(read) => read,
        Err(_) => return false,
    };
    let Some(tree) = trees.iter().find(|tree| tree.id == spec.source_tree) else {
        return false;
    };
    let src = crate::sources::resolve_root(tree, &repository).join(spec.crate_path);
    if !holds_a_crate_root(&src) {
        return false;
    }
    let nested = crate::sources::normalize(Path::new(subject));
    let root = crate::sources::normalize(&src);
    nested.len() > root.len() && nested.starts_with(&root) && nested.as_bytes()[root.len()] == b'/'
}

fn holds_a_crate_root(src: &Path) -> bool {
    src.read_dir()
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry.path().is_file() && entry.path().extension().is_some_and(|ext| ext == "rs")
            })
        })
        .unwrap_or(false)
}

fn declares(subject: &str, spec: &CrateSpec) -> bool {
    let Some(declared) = declared_crate_root(spec) else {
        return false;
    };
    crate::sources::normalize(Path::new(subject)) == declared
}

pub enum Coverage {
    Claimed(&'static CrateSpec),
    Uncovered,
    Orphan(&'static CrateSpec),
}

pub fn cover(crate_src: &str) -> Coverage {
    let mut matches: Vec<&'static CrateSpec> = Vec::new();
    let mut inside: Vec<&'static CrateSpec> = Vec::new();
    for spec in registered() {
        if declares(crate_src, spec) {
            matches.push(spec);
        } else if declared_subdirectory(spec, crate_src) {
            inside.push(spec);
        }
    }
    match matches.len() {
        1 if inside.is_empty() => Coverage::Claimed(matches[0]),
        0 if inside.is_empty() => Coverage::Uncovered,
        1 => Coverage::Orphan(matches[0]),
        _ => {
            let paths: Vec<String> = matches
                .iter()
                .map(|spec| format!("{} in tree `{}`", spec.crate_path, spec.source_tree))
                .collect();
            eprintln!(
                "[lvpa-wrapper] FATAL: {} specs claim the same crate root {crate_src}: {paths:?}. \
                 A crate must be covered by exactly one spec; resolve the overlap before building.",
                matches.len()
            );
            exit(94);
        }
    }
}
