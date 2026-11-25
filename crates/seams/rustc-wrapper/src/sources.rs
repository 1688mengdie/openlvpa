use std::borrow::Cow;
use std::path::{Path, PathBuf};

pub const DECLARATION_ENV: &str = "LVPA_ABSORBED_SOURCES";

pub const DECLARATION_RELATIVE: &str = "scripts/absorbed-sources.json";

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

pub fn workspace_root() -> Option<PathBuf> {
    let mut starts: Vec<PathBuf> = Vec::new();
    if let Ok(working) = std::env::current_dir() {
        starts.push(working);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(directory) = exe.parent() {
            starts.push(directory.to_path_buf());
        }
    }
    for start in starts {
        let start = start.canonicalize().unwrap_or(start);
        let mut cursor = start.as_path();
        loop {
            if cursor.join(REPOSITORY_MARKER).exists() {
                let text = cursor.to_string_lossy();
                return Some(PathBuf::from(strip_verbatim(&text).into_owned()));
            }
            match cursor.parent() {
                Some(parent) => cursor = parent,
                None => break,
            }
        }
    }
    None
}

pub fn repository_root() -> PathBuf {
    workspace_root()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub fn locate() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os(DECLARATION_ENV) {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
        return None;
    }
    for start in [std::env::current_dir().ok(), std::env::current_exe().ok()] {
        let Some(start) = start else {
            continue;
        };
        let start = start.canonicalize().unwrap_or(start);
        let mut candidate = start.as_path();
        loop {
            let path = candidate.join(DECLARATION_RELATIVE);
            if path.is_file() {
                let text = path.to_string_lossy();
                return Some(PathBuf::from(strip_verbatim(&text).into_owned()));
            }
            match candidate.parent() {
                Some(parent) => candidate = parent,
                None => break,
            }
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
