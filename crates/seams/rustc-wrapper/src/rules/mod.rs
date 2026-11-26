use std::path::Path;
use std::process::exit;

use syn::spanned::Spanned;

pub mod composition;
pub mod completion;
pub mod landing;
pub mod rename;

#[path = "openbitfun_computer_use.rs"]
mod openbitfun_computer_use;
#[path = "openbitfun_desktop.rs"]
mod openbitfun_desktop;
#[path = "openalgo_backend.rs"]
mod openalgo_backend;

macro_rules! specs {
    ($($module:ident),+ $(,)?) => {
        mod specs_list {
            $(use $crate::rules::$module;)+

            pub static DECLARED_LIST: &[&'static $crate::rules::CrateSpec] = &[
                $(&$module::SPEC,)+
            ];
        }

        pub use specs_list::DECLARED_LIST;
    };
}

specs!(openbitfun_computer_use, openbitfun_desktop, openalgo_backend);

pub const DECLARATION: &str = "scripts/absorbed-sources.json";

pub struct CrateSpec {
    pub crate_path: &'static str,
    pub source_tree: &'static str,
    pub package: &'static str,
    pub crate_name: &'static str,
    pub package_module: Option<&'static str>,
    pub dependencies: &'static [SeamDependency],
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

fn holds_a_seam_source(path: &str) -> bool {
    let Some(root) = workspace_root_path() else {
        eprintln!(
            "[lvpa-wrapper] FATAL: the workspace root could not be resolved, so the seam source {path} the rule \
             table names cannot be read and nothing tells a seam that landed from one that did not. Nothing is \
             rewritten against a root this pass cannot name."
        );
        exit(97);
    };
    root.join(path).is_file()
}

fn workspace_root_path() -> Option<std::path::PathBuf> {
    crate::sources::workspace_root()
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
    pub seed: SeamSeed,
    pub seeds: &'static [&'static str],
    pub dependencies: &'static [SeamDependency],
}

pub struct SeamDependency {
    pub name: &'static str,
    pub package: &'static str,
    pub version: Option<&'static str>,
    pub path: &'static str,
    pub declared: &'static str,
    pub features: &'static [&'static str],
    pub landing: Landing,
    pub managed: Managed,
}

impl SeamDependency {
    pub fn origin(&self, relative: &str) -> String {
        match self.version {
            Some(version) => format!("version = \"{version}\""),
            None => format!("path = \"{relative}\""),
        }
    }

    pub fn line(&self, relative: &str) -> String {
        let origin = self.origin(relative);
        let mut spec = format!("{{ package = \"{}\", {origin}", self.package);
        if !self.features.is_empty() {
            let features: Vec<String> = self.features.iter().map(|feature| format!("\"{feature}\"")).collect();
            spec.push_str(&format!(", features = [{}]", features.join(", ")));
        }
        spec.push('}');
        format!("{} = {}\n", self.declared_name(), spec)
    }

    pub fn extern_crate(&self) -> Option<String> {
        match self.landing {
            Landing::SeamModule => self.alias(),
            Landing::HostManifest => None,
        }
    }

    pub fn alias(&self) -> Option<String> {
        match self.declared.is_empty() {
            true => self
                .package
                .strip_prefix("lvpa-")
                .map(|_| self.name.replace('-', "_")),
            false => Some(self.name.replace('-', "_")),
        }
    }

    pub fn declared_name(&self) -> String {
        match self.declared.is_empty() {
            false => self.declared.to_string(),
            true => self.alias().unwrap_or_else(|| self.name.to_string()),
        }
    }
}

pub enum Managed {
    Consolidated,
    Replaced,
    Host,
}

impl Managed {
    pub fn adds_an_edge(&self) -> bool {
        matches!(self, Self::Consolidated | Self::Replaced)
    }
}

pub enum Landing {
    HostManifest,
    SeamModule,
}

pub enum SeamSeed {
    Module,
    Imports,
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
    Completion(completion::Completion),
    Rename(rename::Rename),
}

pub enum Landed {
    Rewritten(String),
    NotApplicable,
    AnchorMissing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TextRange {
    start: proc_macro2::LineColumn,
    end: proc_macro2::LineColumn,
}

impl TextRange {
    pub fn of_call(call: &syn::ExprMethodCall) -> Result<Self, String> {
        let mut innermost = call;
        let mut cursor = call.receiver.as_ref();
        while let syn::Expr::MethodCall(nested) = cursor {
            if *nested.method != call.method {
                break;
            }
            innermost = nested;
            cursor = nested.receiver.as_ref();
        }
        Self::between(innermost.receiver.span().start(), call.span().end())
    }

    pub fn between(start: proc_macro2::LineColumn, end: proc_macro2::LineColumn) -> Result<Self, String> {
        match (start.line, end.line) {
            (0, _) | (_, 0) => Err("a seam site carried no position to rewrite".to_string()),
            _ if (end.line, end.column) < (start.line, start.column) => {
                Err("a seam site ended before it began".to_string())
            }
            _ => Ok(Self { start, end }),
        }
    }

    pub fn contains(&self, inner: &Self) -> bool {
        (self.start.line, self.start.column) < (inner.start.line, inner.start.column)
            && (inner.end.line, inner.end.column) <= (self.end.line, self.end.column)
    }

    pub fn after(self, cursor: usize) -> bool {
        self.offset() >= cursor && (self.end.line, self.end.column) > (self.start.line, self.start.column)
    }

    pub fn bytes(self, text: &str) -> Option<(usize, usize)> {
        let start = self.offset();
        let end = self.walk(text);
        if start <= end && text.is_char_boundary(start) && text.is_char_boundary(end) {
            Some((start, end))
        } else {
            None
        }
    }

    fn walk(self, text: &str) -> usize {
        let mut rows = 0usize;
        let mut position = self.start.column;
        for (inner, c) in text.get(self.offset()..).unwrap_or("").chars().enumerate() {
            if inner == self.span() {
                break;
            }
            if c == '\n' {
                rows += 1;
                position = 0;
            } else {
                position += 1;
            }
        }
        (self.start.line - 1 + rows) * LONGEST_LINE + position
    }

    fn span(self) -> usize {
        let rows = self.end.line - self.start.line;
        match rows {
            0 => self.end.column - self.start.column,
            _ => rows * LONGEST_LINE + self.end.column - self.start.column,
        }
    }

    fn offset(self) -> usize {
        (self.start.line - 1) * LONGEST_LINE + self.start.column
    }
}

const LONGEST_LINE: usize = 1_048_576;

fn seam_sources_of(spec: &CrateSpec) -> impl Iterator<Item = &'static SeamDependency> + '_ {
    spec.seams.iter().flat_map(|seam| seam.dependencies)
}

fn reached_edges_land(spec: &CrateSpec) -> Result<(), String> {
    for dependency in spec.dependencies {
        if !dependency.managed.adds_an_edge() {
            continue;
        }
        let reached = seam_sources_of(spec).any(|handed| {
            handed.package == dependency.package && handed.managed.adds_an_edge() && handed.path == dependency.path
        });
        if !reached {
            return Err(format!(
                "the host edge {} lands in {} and points at {}, and no seam of it hands that same crate in under the \
                 name it now stands for",
                dependency.declared_name(),
                spec.package,
                dependency.path
            ));
        }
    }
    Ok(())
}

fn every_landed_name_is_held(seams: &[&Seam]) -> Result<(), String> {
    landing::read_identity()?;
    for seam in seams {
        if !seam.dependencies.iter().any(|dependency| matches!(dependency.managed, Managed::Host)) {
            continue;
        }
        let root = workspace_root_path().ok_or_else(|| "the workspace root could not be resolved".to_string())?;
        let host = root.join(seam.host.as_str_checked());
        landing::hold(seam.dependencies, &seam.seed, seam.seeds, &host)?;
        let owner = landing::owner_of(&host)?;
        for line in seam.dependencies.iter().filter(|dependency| matches!(dependency.managed, Managed::Host)) {
            let version = line.version.ok_or_else(|| {
                format!(
                    "{} lands the seam {} through {}, and an edge written into a manifest by version names the \
                     version it writes",
                    seam.module,
                    owner,
                    line.declared_name()
                )
            })?;
            landing::declare(line.name, &owner, line.package, version)?;
        }
    }
    Ok(())
}

mod decl {
    use std::sync::OnceLock;

    use super::{
        completion, holds_a_seam_source, is_workspace_relative_rust_source, seam_sources_of, CrateSpec,
        Landing, Locate, Managed, Seam, Spec, DECLARATIONS,
    };

    pub struct Declared {
        pub registered: Vec<Spec>,
    }

    static DECLARED: OnceLock<Declared> = OnceLock::new();

    pub fn declared() -> &'static Declared {
        DECLARED.get_or_init(|| {
            every_spec_is_checked(DECLARATIONS.0);
            for spec in DECLARATIONS.0 {
                for seam in spec.seams {
                    assert!(
                        !seam.seeds.is_empty(),
                        "seam {} names no crate root to be declared from, so the module it lands would be unfindable",
                        seam.module
                    );
                    for seed in seam.seeds {
                        assert!(
                            is_workspace_relative_rust_source(seed),
                            "seam {} names the crate root {seed:?}, and a crate root is a crate-relative .rs path",
                            seam.module
                        );
                    }
                    assert!(
                        holds_a_seam_source(seam.host.as_str_checked()),
                        "seam {} names a host this workspace does not hold: {} is not on disk",
                        seam.module,
                        seam.host.as_str_checked()
                    );
                    assert!(
                        holds_a_seam_source(seam.body.as_str_checked()),
                        "seam {} names a body this workspace does not hold: {} is not on disk",
                        seam.module,
                        seam.body.as_str_checked()
                    );
                    for rule in spec.rules {
                        let Locate::Completion(completion) = &rule.locate else {
                            continue;
                        };
                        assert!(
                            completion.seam == seam.module,
                            "rule {} replaces {} on {}, and the seam it lands in is {}",
                            rule.id,
                            completion.method,
                            completion.trait_name,
                            seam.module
                        );
                        if let Err(detail) = completion::declares(completion) {
                            panic!(
                                "seam {} is declared against a body that does not carry it: {detail}",
                                completion.seam
                            );
                        }
                    }
                }
            }
            let seams: Vec<&Seam> = DECLARATIONS.0.iter().flat_map(|spec| spec.seams.iter()).collect();
            if let Err(detail) = super::every_landed_name_is_held(&seams) {
                panic!("a seam reaches its host through a landing this workspace does not settle: {detail}");
            }
            Declared {
                registered: DECLARATIONS.0.iter().map(|spec| Spec::of(spec)).collect(),
            }
        })
    }
    pub fn every_spec_is_checked(specs: &[&'static CrateSpec]) {
        for spec in specs {
            assert!(
                !spec.rules.is_empty(),
                "a crate spec covers only when it carries a rule that lands in its crate"
            );
            if let Err(detail) = super::reached_edges_land(spec) {
                panic!("{detail}");
            }
            assert!(
                !spec.seams.is_empty(),
                "a crate spec covers only when it lands a seam module in its crate"
            );
            for dependency in spec.dependencies {
                if !matches!(dependency.managed, Managed::Replaced) {
                    continue;
                }
                assert!(
                    matches!(dependency.landing, Landing::SeamModule),
                    "the edge {} replaces is a dependency edge, and it is declared to land by {}",
                    dependency.declared_name(),
                    match dependency.landing {
                        Landing::SeamModule => "replacing the edge in the manifest that states it",
                        Landing::HostManifest => "adding a line to the host manifest",
                    }
                );
                assert!(
                    seam_sources_of(spec).any(|reached| {
                        reached.package == dependency.package
                            && reached.managed.adds_an_edge()
                            && reached.path == dependency.path
                    }),
                    "the host edge {} is replaced and points at {}, and no seam of {} hands that same crate in under the name it now stands for",
                    dependency.declared_name(),
                    dependency.path,
                    spec.package
                );
            }
            let _ = seam_sources_of;
            let _ = Managed::Replaced;
        }
    }
}

pub struct Spec(&'static CrateSpec);

impl Spec {
    pub fn of(spec: &'static CrateSpec) -> Self {
        assert!(!spec.rules.is_empty(), "a declared spec carries a rule");
        assert!(!spec.seams.is_empty(), "a declared spec lands a seam");
        Self(spec)
    }

    pub fn crate_spec(&self) -> &'static CrateSpec {
        self.0
    }
}

pub struct Specs(&'static [&'static CrateSpec]);

pub const DECLARATIONS: Specs = Specs(DECLARED_LIST);

pub fn registered() -> Vec<&'static CrateSpec> {
    decl::declared()
        .registered
        .iter()
        .map(Spec::crate_spec)
        .collect()
}

pub fn declaration_state() -> Result<(), String> {
    let specs = registered();
    for spec in &specs {
        for seam in spec.seams {
            if seam.dependencies.is_empty() {
                continue;
            }
            landing::read_identity()?;
        }
    }
    Ok(())
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
    match &rule.locate {
        Locate::Composition(composition) => composition.unit(),
        Locate::Completion(completion) => completion.unit(),
        Locate::Rename(rename) => rename.unit(),
    }
}

pub fn unresolved_declarations() -> Vec<(&'static str, String)> {
    let error = match crate::sources::workspace_root() {
        None => {
            eprintln!(
                "[lvpa-wrapper] FATAL: the workspace root could not be resolved, so neither the absorbed-source \
                 declaration nor the seam source it points at can be found, and nothing tells a seam that landed \
                 from one that did not. Nothing is rewritten."
            );
            exit(97);
        }
        Some(_) => match crate::sources::trees() {
            Ok(_) => return Vec::new(),
            Err(error) => error.to_string(),
        },
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

fn declared_root(spec: &CrateSpec) -> Option<String> {
    let (repository, trees) = crate::sources::trees().ok()?;
    let tree = trees.iter().find(|tree| tree.id == spec.source_tree)?;
    let landed = crate::sources::resolve_root(tree, &repository).join(spec.crate_path);
    if holds_a_crate_root(&landed) {
        return Some(crate::sources::normalize(&landed));
    }
    match landed.parent() {
        Some(parent) if holds_a_crate_root(parent) => Some(crate::sources::normalize(parent)),
        _ => None,
    }
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

fn declared_subdirectory(spec: &CrateSpec, subject: &str) -> bool {
    let Some(root) = declared_root(spec) else {
        return false;
    };
    let nested = crate::sources::normalize(Path::new(subject));
    nested.len() > root.len() && nested.starts_with(&root) && nested.as_bytes()[root.len()] == b'/'
}

fn declares(subject: &str, spec: &CrateSpec) -> bool {
    let Some(declared) = declared_root(spec) else {
        return false;
    };
    let nested = crate::sources::normalize(Path::new(subject));
    nested == declared || nested == format!("{declared}/src")
}

fn within_a_tree(subject: &str) -> bool {
    let Ok((repository, trees)) = crate::sources::trees() else {
        eprintln!(
            "[lvpa-wrapper] FATAL: the rule table's declaration `{DECLARATION}` did not resolve, so whether {subject} \
             is a crate this pass covers cannot be told, and every seam would read as one that need not land. \
             Nothing is rewritten against an unresolved table."
        );
        exit(97);
    };
    let nested = crate::sources::normalize(Path::new(subject));
    trees.iter().any(|tree| {
        let root = crate::sources::normalize(&crate::sources::resolve_root(tree, &repository));
        nested.len() > root.len() && nested.starts_with(&root) && nested.as_bytes()[root.len()] == b'/'
    })
}

pub enum Coverage {
    Claimed(&'static CrateSpec),
    Absorbed,
    Uncovered,
    Orphan(&'static CrateSpec),
    Ruleless(&'static CrateSpec),
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
        1 if inside.is_empty() => match matches[0].rules.is_empty() {
            true => Coverage::Ruleless(matches[0]),
            false => Coverage::Claimed(matches[0]),
        },
        0 if inside.is_empty() => {
            if within_a_tree(crate_src) {
                Coverage::Absorbed
            } else {
                Coverage::Uncovered
            }
        }
        1 if matches[0].crate_path.ends_with("/src") => Coverage::Orphan(matches[0]),
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
