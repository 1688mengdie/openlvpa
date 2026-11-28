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
pub mod relay_service;

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

specs!(
    openbitfun_computer_use,
    openbitfun_desktop,
    openalgo_backend,
    relay_service
);

pub const DECLARATION: &str = "scripts/absorbed-sources.json";

pub struct CrateSpec {
    pub crate_path: &'static str,
    pub source_tree: &'static str,
    pub package: &'static str,
    pub crate_name: &'static str,
    pub package_module: Option<&'static str>,
    pub root: Option<&'static str>,
    pub dependencies: &'static [SeamDependency],
    pub seams: &'static [Seam],
    pub rules: &'static [Rule],
    pub carries: &'static [Carry],
}

pub struct SeamHost(&'static str);

pub struct SeamBody(&'static str);

impl SeamHost {
    /// A seam host is a crate: the directory whose manifest names it. A seam lands in a crate of
    /// this repository, and the crate is the subject the rules rewrite only while it is the one
    /// this pass compiles -- which is never the seam crate itself.
    pub const fn checked(path: &'static str) -> Self {
        assert!(
            is_workspace_relative_crate(path),
            "a seam host must be a workspace-root-relative crate directory"
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
            is_workspace_relative_crate(path),
            "a seam body must be a workspace-root-relative crate directory"
        );
        Self(path)
    }

    pub const fn as_str_checked(&self) -> &'static str {
        self.0
    }
}

const fn is_workspace_relative_crate(path: &str) -> bool {
    let bytes = path.as_bytes();
    if bytes.is_empty() || bytes[0] == b'/' || bytes[0] == b'\\' || (bytes.len() >= 2 && bytes[1] == b':') {
        return false;
    }
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'.' && bytes[index + 1] == b'.' {
            return false;
        }
        index += 1;
    }
    true
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
    let at = root.join(path);
    // a host is the access the seam is held through: one file of the crate, or the source
    // directory a plain module is stated from
    at.is_file() || (at.is_dir() && !path.ends_with(".rs"))
}

fn workspace_root_path() -> Option<std::path::PathBuf> {
    crate::sources::workspace_root()
}

/// The file the types a landed call states are held in. A type the entry takes beside its statement
/// is written where the crate can read it, and the place is the crate's own source directory beside
/// a seam body the crate is built with, so the name the seam states is the only name the file
/// carries and two crates of the workspace hold two files.
pub fn rows_file_of(seam: &str) -> Option<std::path::PathBuf> {
    let seam_body = registered()
        .iter()
        .flat_map(|spec| spec.seams.iter())
        .find(|body| body.module == seam)
        .map(|body| body.host.as_str_checked().to_string())?;
    let at = workspace_root_path()?.join(seam_body);
    Some(at.with_file_name("lvpa_rows_of_the_program.rs"))
}

/// The types the program states are held beside the crate the rewrite lands in, as the program
/// wrote them: the entry takes one of them, so every one of them reads in the crate the entry is
/// reached from.
pub fn hold_the_rows(seam: &str, rows: &str) {
    let Some(at) = rows_file_of(seam) else {
        return;
    };
    let held = std::fs::read_to_string(&at).unwrap_or_default();
    if held.trim() == rows.trim() {
        return;
    }
    if let Some(parent) = at.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&at, rows.trim_end() .to_string() + "\n");
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
    pub features: &'static [String],
    pub stands_down: bool,
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
        format!("{} = {}", self.declared_name(), spec)
    }

    pub fn extern_crate(&self) -> Option<String> {
        match self.landing {
            Landing::SeamModule => self.alias(),
            Landing::HostManifest | Landing::MemberManifest => None,
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
    MemberManifest,
}

impl PartialEq for Landing {
    fn eq(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Landing::HostManifest, Landing::HostManifest)
                | (Landing::SeamModule, Landing::SeamModule)
                | (Landing::MemberManifest, Landing::MemberManifest)
        )
    }
}

pub enum SeamSeed {
    Module,
    Imports,
}

/// A statement the tree's source states today and the series this repository compiles it at
/// states otherwise: the text is the tree's own line, word for word, and it is carried to the text
/// that series reads. The pass rewrites one statement per entry and says so when the line is not
/// there, so a source that moved stops the pass instead of compiling against half a carry.
pub struct Carry {
    pub id: &'static str,
    pub file: &'static str,
    pub from: &'static str,
    pub to: &'static str,
}

pub struct Rule {
    pub id: &'static str,
    pub file: &'static str,
    /// The attribute that opens a block of this file the rule holds back from: a statement the
    /// program writes for its own tests is not a statement the crate compiles, so a rule that reads
    /// the crate holds back from the block rather than reaching into it.
    pub hold_back_in: Option<&'static str>,
    pub locate: Locate,
}

pub struct Carries {
    pub statements: &'static [Carry],
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
    Surround(rename::Surround),
}

impl Locate {
    /// The spelling a rule is written against, named back for whoever has to read why it did
    /// not resolve.
    pub fn anchor(&self) -> &str {
        match self {
            Locate::Composition(_) => "its envelope",
            Locate::Completion(completion) => completion.exports,
            Locate::Rename(rename) => rename.from,
            Locate::Surround(surround) => surround.from,
        }
    }
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
    /// The line the place begins on, in the text the place was read on.
    pub fn line(self) -> usize {
        self.start.line
    }

    pub fn of_call(call: &syn::ExprMethodCall) -> Result<Self, String> {
        let mut innermost = call;
        let mut cursor = call.receiver.as_ref();
        while let syn::Expr::MethodCall(nested) = cursor {
            if nested.method != call.method {
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

    /// The byte a position names, read against the text it was taken from: a place is compared
    /// against the names a walk of the same syntax states, and the two are one measure only when
    /// both are read as bytes of that text.
    pub fn at(at: proc_macro2::LineColumn, text: &str) -> Option<usize> {
        at_text(at, text)
    }

    pub fn contains(&self, inner: &Self) -> bool {
        (self.start.line, self.start.column) < (inner.start.line, inner.start.column)
            && (inner.end.line, inner.end.column) <= (self.end.line, self.end.column)
    }

    pub fn after(self, cursor: usize) -> bool {
        (self.start.line - 1) * LOWEST_LINE + self.start.column >= cursor && (self.end.line, self.end.column) > (self.start.line, self.start.column)
    }

    /// The byte range this site names, read against the text it was taken from: a line and a
    /// column is a position in that text, not an index into a fixed-width guess at it, so the
    /// range is walked out of the text itself and refused when it names no character boundary.
    /// The span of the shape that reaches the end of the text the call carries: the site is the
    /// span the shape states, and a call states its argument after it, so the site is widened by
    /// the text the argument is written by.


    pub fn widened(self, width: usize) -> Self {
        Self {
            end: proc_macro2::LineColumn {
                line: self.end.line,
                column: self.end.column + width,
            },
            start: self.start
        }
    }

    pub fn bytes(self, text: &str) -> Option<(usize, usize)> {
        let start = at_text(self.start, text)?;
        let end = at_text(self.end, text)?;
        if start <= end && text.is_char_boundary(start) && text.is_char_boundary(end) {
            Some((start, end))
        } else {
            None
        }
    }


}

/// The byte offset a line and a column name in this text, walked out of the text itself.
pub(crate) fn at_text(position: proc_macro2::LineColumn, text: &str) -> Option<usize> {
    let mut at = 0usize;
    for _ in 1..position.line {
        at += text.get(at..)?.find('\n')? + 1;
    }
    let at = at + position.column;
    if at <= text.len() && text.is_char_boundary(at) {
        Some(at)
    } else {
        None
    }
}


const LOWEST_LINE: usize = 1_048_576;

fn seam_sources_of(spec: &CrateSpec) -> impl Iterator<Item = &'static SeamDependency> + '_ {
    spec.seams.iter().flat_map(|seam| seam.dependencies)
}

fn reached_edges_land(spec: &CrateSpec) -> Result<(), String> {
    for dependency in spec.dependencies {
        if !matches!(dependency.managed, Managed::Consolidated) {
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
    for dependency in spec.dependencies {
        if !matches!(dependency.managed, Managed::Replaced) {
            continue;
        }
        if dependency.landing != Landing::MemberManifest {
            continue;
        }
        if dependency.version.is_none() || dependency.declared.is_empty() {
            return Err(format!(
                "the member edge {} is landed in its own card, and an edge landed there names both the key that card \
                 writes and the range it is landed at",
                dependency.package
            ));
        }
    }
    Ok(())
}

fn every_landed_name_is_held(seams: &[&'static Seam]) -> Result<(), String> {
    // the pass that settles the cards runs before any crate is compiled, and it settles every seam
    // of every card: a run that compiles one crate -- a dependency of this repository itself --
    // carries the seams of no card, and its landing table is not read here
    if seams.is_empty() {
        return Ok(());
    }
    if !seams.iter().any(|seam| {
        seam.dependencies
            .iter()
            .any(|dependency| !matches!(dependency.managed, Managed::Host))
    }) {
        return Ok(());
    }
    for seam in seams {
        let rooted = seam.dependencies.iter().any(|dependency| matches!(dependency.managed, Managed::Host));
        let keyed = seam.dependencies.iter().any(|dependency| !matches!(dependency.managed, Managed::Host));
        if !rooted && !keyed {
            continue;
        }
        let root = workspace_root_path().ok_or_else(|| "the workspace root could not be resolved".to_string())?;
        let host = root.join(seam.host.as_str_checked());
        crate::rewrite::lay_seam(&host, seam)?;
        let owner = landing::owner_of(&host)?;
        let identity = landing::identity()?;
        for line in seam.dependencies.iter().filter(|dependency| matches!(dependency.managed, Managed::Host)) {
            if line.version.is_none() && !line.path.starts_with("crates/seams/") {
                return Err(format!(
                    "{} lands the seam {} through {}, and an edge written into a manifest by version names the \
                     version it writes",
                    seam.module,
                    owner,
                    line.declared_name()
                ));
            }
            if let (Some(version), Some(named)) = (line.version, identity.iter().find(|named| named.index == line.name)) {
                if owner != named.owner || line.package != named.package || version != named.version {
                    return Err(format!(
                        "{owner} reaches {} {version}, and the entry the tree lands under states {} {} beside {}",
                        line.package, named.package, named.version, named.owner
                    ));
                }
            }
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
                            is_workspace_relative_rust_source(seed) || *seed == seam.module,
                            "seam {} names the crate root {seed:?}, and a crate root is a crate-relative .rs path \
                             or the name of the module it lands",
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
                    for seam in spec.seams {
                        for seed in seam.seeds {
                            let names_the_seam = *seed == seam.module;
                            let names_a_file = seed.ends_with(".rs") && !seed.contains('/');
                            assert!(
                                names_the_seam || names_a_file,
                                "seam {} names the crate root {seed:?}, and a crate root is a .rs file of the crate this seam is laid in or the name of the module it lands",
                                seam.module
                            );
                        }
                    }
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
            let mut seams: Vec<&'static Seam> = Vec::new();
            for spec in DECLARATIONS.0 {
                for seam in spec.seams {
                    seams.push(seam);
                }
            }
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
                if matches!(dependency.landing, Landing::MemberManifest | Landing::HostManifest) {
                    continue;
                }
                assert!(
                    matches!(dependency.landing, Landing::SeamModule),
                    "the edge {} replaces is a dependency edge, and it is declared to land by {}",
                    dependency.declared_name(),
                    match dependency.landing {
                        Landing::SeamModule => "replacing the edge in the manifest that states it",
                        Landing::HostManifest => "adding a line to the host manifest",
                        Landing::MemberManifest => "rewriting the edge in the member's own card",
                    }
                );
                assert!(
                    seam_sources_of(spec).any(|reached| {
                        let same_path = match (reached.path.is_empty(), dependency.path.is_empty()) {
                            (true, true) => true,
                            (false, false) => reached.path == dependency.path,
                            (one_empty, _) => !one_empty,
                        };
                        reached.package == dependency.package
                            && reached.managed.adds_an_edge()
                            && same_path
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

/// The card one crate states its own edges in: the manifest of its source tree's member that
/// calls itself by that package. A tree's members are read out of the manifest the take keeps of
/// it -- the tree's own card from before the strip -- because the card standing in the tree after
/// the take is the host's, and a member list read there names the host's members.
pub fn card_of(spec: &CrateSpec) -> Option<std::path::PathBuf> {
    let (repository, trees) = crate::sources::trees().ok()?;
    let tree = trees.iter().find(|tree| tree.id == spec.source_tree)?;
    let root = crate::sources::resolve_root(tree, &repository);
    let text = std::fs::read_to_string(snapshot_of(&repository, tree)).ok()?;
    for member in crate::sources::members_of(&text) {
        let card = root.join(&member).join("Cargo.toml");
        let Ok(body) = std::fs::read_to_string(&card) else {
            continue;
        };
        if body.lines().any(|line| line.trim() == format!("name = \"{}\"", spec.package)) {
            return Some(card);
        }
    }
    None
}

/// The manifest a tree's members are read out of: the take keeps the tree's own card beside the
/// declaration, and the entry that names the tree names the file.
fn snapshot_of(repository: &Path, tree: &crate::sources::SourceTree) -> std::path::PathBuf {
    let declaration = crate::sources::locate()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    let mut inside = false;
    for line in declaration.lines() {
        let trimmed = line.trim();
        if trimmed == format!("\"id\": \"{}\",", tree.id) {
            inside = true;
            continue;
        }
        if inside {
            if let Some(rest) = trimmed.strip_prefix("\"snapshotFile\": \"") {
                if let Some(path) = rest.split('"').next() {
                    return repository.join(path);
                }
            }
            if trimmed.starts_with('}') {
                inside = false;
            }
        }
    }
    repository.join(&tree.root).join("Cargo.toml")
}

pub fn registered() -> Vec<&'static CrateSpec> {
    decl::declared()
        .registered
        .iter()
        .map(Spec::crate_spec)
        .collect()
}

/// The card one absorbed tree's member states a given key in, read out of the tree's
/// members as the tree's own root lists them. A key more than one member states is a key
/// this pass cannot settle in one card, and it stops here.
pub fn member_card(key: &str) -> Option<std::path::PathBuf> {
    let (repository, trees) = crate::sources::trees().ok()?;
    let mut standing = Vec::new();
    for tree in &trees {
        let root = crate::sources::resolve_root(tree, &repository);
        let Ok(text) = std::fs::read_to_string(root.join("Cargo.toml")) else {
            continue;
        };
        for name in crate::sources::members_of(&text) {
            let card = root.join(&name).join("Cargo.toml");
            let Ok(body) = std::fs::read_to_string(&card) else {
                continue;
            };
            let states = body
                .lines()
                .any(|line| crate::rewrite::edge_key_of(line) == Some(key));
            if states && !standing.contains(&card) {
                standing.push(card);
            }
        }
    }
    match standing.len() {
        1 => standing.pop(),
        _ => None,
    }
}

/// Whether one crate root is the crate a spec lands in: the card a crate root reads is the
/// one the spec names, and a crate this pass lands an edge in is a crate whose card this
/// pass reads whether or not the compiler was handed a `.rs` file of it.
/// The crates this pass lands an edge or a seam in, by the package that card states. An
/// absorbed tree's crates stand under one tree, so a spec names the package its own card
/// declares and no path is ever spelled twice: the card is the identity, and a crate path
/// only tells one card from another inside a tree.
pub fn lands_in(spec: &CrateSpec, crate_root: &str) -> bool {
    let Some(root) = crate::sources::workspace_root() else {
        return false;
    };
    let subject = crate::sources::normalize(Path::new(crate_root));
    let cut = subject.trim_end_matches("/src").to_string();
    let Some(parent) = Path::new(crate_root).parent() else {
        return false;
    };
    let Ok(body) = std::fs::read_to_string(parent.join("Cargo.toml")) else {
        return false;
    };
    let _ = root;
    let _ = cut;
    let Some(name) = body
        .lines()
        .find_map(|line| line.trim().strip_prefix("name = "))
    else {
        return false;
    };
    name.trim().trim_matches('"') == spec.package
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
        Locate::Surround(surround) => surround.unit(),
    }
}

pub fn spec_of_crate_root(root: &Path) -> Option<&'static CrateSpec> {
    let root = crate::sources::normalize(root);
    registered()
        .into_iter()
        .find(|spec| declared_root(spec).is_some_and(|declared| declared == root))
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
    let repository = crate::sources::repository_root();
    let (_, trees) = crate::sources::trees().ok()?;
    let tree = trees.iter().find(|tree| tree.id == spec.source_tree)?;
    let landed = crate::sources::resolve_root(tree, &repository).join(spec.crate_path);
    match spec.root {
        Some(root) => Some(crate::sources::normalize(&landed.join(root))),
        None => holds_a_crate_root(&landed).then(|| crate::sources::normalize(&landed)),
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
