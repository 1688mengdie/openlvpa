use std::path::Path;

const IDENTITY: &str = "scripts/landing-identity.toml";

pub struct Named {
    pub index: String,
    pub owner: String,
    pub declared: String,
    pub package: String,
    pub version: String,
}

fn field(line: &str, key: &str) -> Option<String> {
    let value = line.trim().strip_prefix(key)?.trim_start().strip_prefix('=')?.trim();
    Some(value.trim_matches(|one| one == '"' || one == '\'').to_string())
}

fn named(text: &str) -> Vec<Named> {
    let mut out: Vec<Named> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            out.push(Named {
                index: String::new(),
                owner: String::new(),
                declared: String::new(),
                package: String::new(),
                version: String::new(),
            });
            continue;
        }
        let Some(entry) = out.last_mut() else {
            continue;
        };
        for key in ["index", "owner", "declared", "package", "version"] {
            if let Some(value) = field(line, key) {
                match key {
                    "index" => entry.index = value,
                    "owner" => entry.owner = value,
                    "declared" => entry.declared = value,
                    "package" => entry.package = value,
                    _ => entry.version = value,
                }
            }
        }
    }
    out
}

pub fn identity() -> Result<Vec<Named>, String> {
    Ok(named(&read_identity()?))
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| {
        format!(
            "{} is not readable as the identity the members this tree absorbs are landed under: {error}",
            path.display()
        )
    })
}

fn identity_manifest() -> Result<std::path::PathBuf, String> {
    Ok(crate::sources::workspace_root()
        .ok_or_else(|| {
            format!(
                "the workspace root is unresolvable, so {IDENTITY} cannot be read as the landing identity every \
                 crate this tree lands under is declared to carry"
            )
        })?
        .join(IDENTITY))
}

pub fn read_identity() -> Result<String, String> {
    let path = identity_manifest()?;
    read(&path)
}

fn crate_root_of(host: &Path) -> Result<std::path::PathBuf, String> {
    let root = host
        .ancestors()
        .find(|at| at.join("Cargo.toml").is_file())
        .ok_or_else(|| format!("{} sits under no crate root", host.display()))?;
    Ok(root.to_path_buf())
}

pub fn owner_of(host: &Path) -> Result<String, String> {
    let manifest = crate_root_of(host)?.join("Cargo.toml");
    let text = read(&manifest)?;
    text.lines()
        .find_map(|line| line.trim().strip_prefix("name = \""))
        .and_then(|rest| rest.split('"').next())
        .map(str::to_string)
        .ok_or_else(|| format!("{} names no package", manifest.display()))
}

