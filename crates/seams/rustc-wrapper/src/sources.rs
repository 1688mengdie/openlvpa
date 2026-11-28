use std::borrow::Cow;
use std::path::{Path, PathBuf};

pub const DECLARATION_ENV: &str = "LVPA_ABSORBED_SOURCES";

pub const REPOSITORY_ENV: &str = "LVPA_REPO_ROOT";

/// The marker a run sets for the compilers it starts: it says the root was settled above and is
/// not settled again below, so a crate of the registry reads the declaration instead of inheriting
/// the directory an enclosing run happened to name.
pub const RESOLVED_ENV: &str = "LVPA_ROOT_RESOLVED";

pub const DECLARATION_RELATIVE: &str = "scripts/absorbed-sources.json";

/// The one file a run leaves beside the binary it is: a compiler this binary starts reaches that
/// directory through its own executable, so the root is read there and nowhere a compiler of the
/// registry could lose it. The file states the root the run was resolved against.
pub const ROOT_BESIDE: &str = "lvpa-root.txt";

pub const REPOSITORY_MARKER: &str = ".git";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceTree {
    pub id: String,
    pub root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourcesError {
    Missing { path: PathBuf },
    Unreadable { path: PathBuf, detail: String },
    NoProjects { path: PathBuf },
    IncompleteEntry { path: PathBuf, index: usize },
    DuplicateId { value: String },
    DuplicateRoot { value: String },
}

impl std::fmt::Display for SourcesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { path } => write!(
                f,
                "the absorbed-source declaration is missing: {}",
                path.display()
            ),
            Self::Unreadable { path, detail } => write!(
                f,
                "the absorbed-source declaration {} could not be read: {detail}",
                path.display()
            ),
            Self::NoProjects { path } => write!(
                f,
                "{} declares no projects, so there is no tree to resolve against",
                path.display()
            ),
            Self::IncompleteEntry { path, index } => write!(
                f,
                "entry {index} of {} carries no `id` and `root`, so it names no tree",
                path.display()
            ),
            Self::DuplicateId { value } => write!(
                f,
                "two projects declare the id `{value}`, so a crate naming it cannot be resolved"
            ),
            Self::DuplicateRoot { value } => write!(
                f,
                "two projects declare the root `{value}`, so a lookup by directory cannot choose"
            ),
        }
    }
}

/// The root this run was named: the directory handed in here decides where this pass lands and
/// where it reads, and the file beside the binary is what a compiler of the registry reads when a
/// run named it nothing. A file left by a run above states that run's root, so it answers only
/// where this run names none: a stale word taken first moves every landing and every reading to
/// another tree, and the cards of the root this run names are then read by no pass at all.
pub fn declared_repository_root() -> Option<PathBuf> {
    if let Some(named) = named_repository_root() {
        return Some(named);
    }
    root_beside_the_binary()
}

fn named_repository_root() -> Option<PathBuf> {
    let text = std::env::var_os(REPOSITORY_ENV)?;
    if text.is_empty() {
        return None;
    }
    let path = PathBuf::from(text);
    if !path.is_dir() {
        return None;
    }
    let canonical = path.canonicalize().unwrap_or(path);
    Some(PathBuf::from(strip_verbatim(&canonical.to_string_lossy()).into_owned()))
}

pub fn workspace_root() -> Option<PathBuf> {
    if let Some(declared) = declared_repository_root() {
        return Some(declared);
    }
    let mut starts: Vec<PathBuf> = Vec::new();
    if let Some(declaration) = locate() {
        let root = repository_of(&declaration);
        if root.is_dir() {
            starts.push(root);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(directory) = exe.parent() {
            starts.push(directory.to_path_buf());
        }
    }
    if let Ok(working) = std::env::current_dir() {
        starts.push(working);
    }
    for start in starts {
        let start = start.canonicalize().unwrap_or(start);
        let mut cursor = start.as_path();
        loop {
            if cursor.join(REPOSITORY_MARKER).exists() || cursor.join(DECLARATION_RELATIVE).is_file() {
                let text = cursor.to_string_lossy();
                return Some(PathBuf::from(strip_verbatim(&text).into_owned()));
            }
            match cursor.parent() {
                Some(parent) => cursor = parent,
                None => break,
            }
        }
    }
    if route_is_settled() {
        eprintln!(
            "[lvpa-wrapper] FATAL: the run states its repository, and neither that directory nor any directory above it \
             is a repository. Nothing is rewritten against a root this pass cannot name."
        );
        std::process::exit(97);
    }
    None
}

/// Whether the root was handed to this run rather than searched for: a run that was handed one and
/// finds no repository above it says so instead of reading the tree as one nothing is declared for.
fn route_is_settled() -> bool {
    std::env::var_os(REPOSITORY_ENV).is_some_and(|text| !text.is_empty())
}

pub fn repository_root() -> PathBuf {
    workspace_root().unwrap_or_else(|| {
        eprintln!(
            "[lvpa-wrapper] FATAL: the workspace root could not be resolved, so no path this pass writes can be \
             named relative to it. Nothing is rewritten."
        );
        std::process::exit(97);
    })
}

/// The root a run stated beside its own binary. Every compiler this binary starts reaches that
/// directory through its own executable, so the root survives a start whose working directory and
/// inherited environment belong to a crate of the registry.
fn root_beside_the_binary() -> Option<PathBuf> {
    let beside = std::env::current_exe().ok()?.parent()?.join(ROOT_BESIDE);
    let text = std::fs::read_to_string(&beside).ok()?;
    let path = PathBuf::from(text.trim());
    if path.is_dir() {
        return Some(path);
    }
    eprintln!(
        "[lvpa-wrapper] FATAL: {} states {path:?} as the workspace root, and that directory does not stand. The \
         root a run settles is read by every compiler it starts, so a root that no longer stands is no root. \
         Nothing is rewritten.",
        beside.display()
    );
    std::process::exit(97);
}

/// State the root beside this binary: the file is what a compiler started from here reads.
pub fn state_the_root_beside_the_binary(root: &Path) {
    let Ok(executable) = std::env::current_exe() else {
        eprintln!(
            "[lvpa-wrapper] FATAL: the executable cannot be named, so the root {root:?} a compiler of the registry \
             reads stands nowhere. Nothing is rewritten."
        );
        std::process::exit(97);
    };
    let Some(directory) = executable.parent() else {
        eprintln!(
            "[lvpa-wrapper] FATAL: {} sits in no directory, so the root {root:?} a compiler of the registry reads \
             stands nowhere. Nothing is rewritten.",
            executable.display()
        );
        std::process::exit(97);
    };
    let beside = directory.join(ROOT_BESIDE);
    let _ = std::fs::remove_file(&beside);
    if let Err(error) = std::fs::write(&beside, root.to_string_lossy().as_ref()) {
        eprintln!(
            "[lvpa-wrapper] FATAL: {} cannot be written, so the root {root:?} a compiler of the registry reads \
             stands nowhere: {error}",
            beside.display()
        );
        std::process::exit(97);
    }
}

pub fn locate() -> Option<PathBuf> {
    // the statement this pass reads is the one its own repository states: a pointed declaration is
    // honoured only where it is the statement of a repository, not a path copied out of another
    if let Some(explicit) = std::env::var_os(DECLARATION_ENV) {
        let path = PathBuf::from(explicit);
        let held = repository_of(&path);
        if path.is_file() && declaration_of(&held).is_some() {
            return Some(path);
        }
        eprintln!(
            "[lvpa-wrapper] FATAL: {DECLARATION_ENV} states {}, and that file is not the declaration of the \
             repository it names. Nothing is rewritten against a declaration this pass cannot read.",
            path.display()
        );
        std::process::exit(97);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(directory) = exe.parent() {
            let mut cursor = directory;
            loop {
                let path = cursor.join(DECLARATION_RELATIVE);
                if path.is_file() {
                    let text = path.to_string_lossy();
                    return Some(PathBuf::from(strip_verbatim(&text).into_owned()));
                }
                match cursor.parent() {
                    Some(parent) => cursor = parent,
                    None => break,
                }
            }
        }
    }
    let Some(working) = std::env::current_dir().ok() else {
        return None;
    };
    let mut candidate = working.canonicalize().unwrap_or(working);
    loop {
        let path = candidate.join(DECLARATION_RELATIVE);
        if path.is_file() {
            let text = path.to_string_lossy();
            return Some(PathBuf::from(strip_verbatim(&text).into_owned()));
        }
        match candidate.parent() {
            Some(parent) => candidate = parent.to_path_buf(),
            None => break,
        }
    }
    None
}

pub fn repository_of(declaration: &Path) -> PathBuf {
    declaration
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(repository_root)
}

/// The declaration a directory states for itself, named by the position this repository fixes and
/// by nothing the directory happens to hold.
fn declaration_of(repository: &Path) -> Option<PathBuf> {
    let path = repository.join(DECLARATION_RELATIVE);
    path.is_file().then_some(path)
}

pub fn trees() -> Result<(PathBuf, Vec<SourceTree>), SourcesError> {
    let path = locate().ok_or_else(|| SourcesError::Missing {
        path: repository_root().join(DECLARATION_RELATIVE),
    })?;
    let text = std::fs::read_to_string(&path).map_err(|error| SourcesError::Unreadable {
        path: path.clone(),
        detail: error.to_string(),
    })?;
    let trees = parse_projects(&text, &path)?;
    Ok((repository_of(&path), trees))
}

fn parse_projects(text: &str, path: &Path) -> Result<Vec<SourceTree>, SourcesError> {
    let entries = projects_array(text);
    if entries.is_empty() {
        return Err(SourcesError::NoProjects {
            path: path.to_path_buf(),
        });
    }
    let mut trees: Vec<SourceTree> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let id = string_value(entry, "id").ok_or_else(|| SourcesError::IncompleteEntry {
            path: path.to_path_buf(),
            index,
        })?;
        let root = string_value(entry, "root").ok_or_else(|| SourcesError::IncompleteEntry {
            path: path.to_path_buf(),
            index,
        })?;
        if trees.iter().any(|tree| tree.id == id) {
            return Err(SourcesError::DuplicateId { value: id });
        }
        if trees.iter().any(|tree| tree.root == root) {
            return Err(SourcesError::DuplicateRoot { value: root });
        }
        trees.push(SourceTree { id, root });
    }
    Ok(trees)
}

fn projects_array(text: &str) -> Vec<String> {
    let Some(start) = find_key(text, "projects") else {
        return Vec::new();
    };
    let Some(open) = text[start..].find('[').map(|at| start + at) else {
        return Vec::new();
    };
    let Some(close) = matching(text, open, '[', ']') else {
        return Vec::new();
    };
    objects(&text[open + 1..close])
}

fn objects(body: &str) -> Vec<String> {
    let bytes: Vec<char> = body.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != '{' {
            index += 1;
            continue;
        }
        let Some(end) = matching_chars(&bytes, index, '{', '}') else {
            break;
        };
        out.push(bytes[index..=end].iter().collect());
        index = end + 1;
    }
    out
}

fn find_key(text: &str, key: &str) -> Option<usize> {
    let needle = format!("\"{key}\"");
    let mut from = 0usize;
    while let Some(at) = text[from..].find(&needle).map(|found| from + found) {
        let after = at + needle.len();
        if text[after..].trim_start().starts_with(':') {
            return Some(after);
        }
        from = after;
    }
    None
}

fn string_value(object: &str, key: &str) -> Option<String> {
    let at = find_key(object, key)?;
    let rest = object[at..].trim_start();
    let rest = rest.strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => out.push(other),
                None => return None,
            },
            other => out.push(other),
        }
    }
    None
}

fn matching(text: &str, open: usize, start: char, end: char) -> Option<usize> {
    let bytes: Vec<char> = text.chars().collect();
    matching_chars(&bytes, text[..open].chars().count(), start, end)
}

fn matching_chars(bytes: &[char], open: usize, start: char, end: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = open;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let c = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if c == '"' {
            in_string = true;
        } else if c == start {
            depth += 1;
        } else if c == end {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
        index += 1;
    }
    None
}

/// The member directories one tree's own root card lists, read as that card writes them.
pub fn members_of(text: &str) -> Vec<String> {
    let mut members = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if members_opened(trimmed) {
            inside = true;
            continue;
        }
        if !inside {
            continue;
        }
        if trimmed.starts_with(']') {
            inside = false;
            continue;
        }
        if let Some(name) = trimmed.split('"').nth(1) {
            members.push(name.to_string());
        }
    }
    members
}

/// The line that opens the members array, read at whatever indentation the card writes it: the
/// anchor is the key and the bracket, not the spacing a card happens to use.
fn members_opened(trimmed: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix("members") else {
        return false;
    };
    let Some(rest) = rest.trim_start().strip_prefix('=') else {
        return false;
    };
    rest.trim_start().starts_with('[')
}

pub fn resolve_root(tree: &SourceTree, repository: &Path) -> PathBuf {
    let root = Path::new(&tree.root);
    if root.is_absolute() {
        return root.to_path_buf();
    }
    repository.join(root)
}

const VERBATIM_PREFIX: &str = r"\\?\";
const VERBATIM_UNC_PREFIX: &str = r"UNC\";

fn strip_verbatim(text: &str) -> Cow<'_, str> {
    let Some(rest) = text.strip_prefix(VERBATIM_PREFIX) else {
        return Cow::Borrowed(text);
    };
    match rest.strip_prefix(VERBATIM_UNC_PREFIX) {
        Some(authority) => Cow::Owned(format!(r"\\{authority}")),
        None => Cow::Borrowed(rest),
    }
}

pub fn normalize(path: &Path) -> String {
    let lossy = path.to_string_lossy();
    let text = strip_verbatim(&lossy).replace('\\', "/");
    let trimmed = text.trim_end_matches('/').to_string();
    if cfg!(windows) {
        trimmed.to_lowercase()
    } else {
        trimmed
    }
}
