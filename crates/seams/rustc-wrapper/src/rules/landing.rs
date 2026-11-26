use std::path::Path;

use super::{SeamDependency, SeamSeed};

const IDENTITY: &str = "scripts/landing-identity.toml";

pub struct Named {
    pub index: &'static str,
    pub owner: &'static str,
    pub declared: &'static str,
    pub package: &'static str,
    pub version: &'static str,
}

pub const NAMES: &[Named] = &[Named {
    index: "windows",
    owner: "openbitfun-computer-use",
    declared: "windows",
    package: "windows",
    version: "0.62",
}];

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
    let crate_root = host
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .ok_or_else(|| format!("{} names no crate root to read a manifest from", host.display()))?;
    Ok(crate_root.to_path_buf())
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

pub fn crate_root_names(host: &Path) -> Result<String, String> {
    let root = crate_root_of(host)?.join("src/lib.rs");
    read(&root)
}

pub fn hold(
    dependencies: &[SeamDependency],
    seed: &SeamSeed,
    seeds: &[&str],
    host: &Path,
) -> Result<(), String> {
    for dependency in dependencies {
        if dependency.managed.adds_an_edge() {
            return Err(format!(
                "the seam reaches {} by adding an edge into the crate root it lands in; landing {} states every \
                 edge that crate carries, and no second line is added beside it",
                dependency.package,
                read_identity()?
            ));
        }
    }
    if matches!(seed, SeamSeed::Imports) && !dependencies.is_empty() {
        return Err(format!(
            "the seam arrives through an import, and the import it would have to arrive over is the {} this landing \
             already names",
            dependencies
                .iter()
                .map(SeamDependency::declared_name)
                .collect::<Vec<String>>()
                .join(", ")
        ));
    }
    if matches!(seed, SeamSeed::Module) {
        let body = crate_root_names(host)?;
        for name in seeds {
            if !body.contains(&format!("mod {name}")) {
                return Err(format!(
                    "{} declares no `mod {name}` at the crate root it lands in, so the module this seam hands in \
                     would be unfindable there",
                    host.display()
                ));
            }
        }
    }
    Ok(())
}

pub fn declare(
    index: &'static str,
    owner: &str,
    package: &'static str,
    version: &'static str,
) -> Result<&'static Named, String> {
    let Some(named) = NAMES.iter().find(|named| named.index == index) else {
        return Err(format!(
            "{owner} reaches {package} and the landing identity carries no entry {index}"
        ));
    };
    if owner != named.owner || package != named.package || version != named.version {
        return Err(format!(
            "{owner} reaches {package} {version}, and the entry this tree lands under states {} {} beside {}",
            named.package, named.version, named.owner
        ));
    }
    Ok(named)
}
