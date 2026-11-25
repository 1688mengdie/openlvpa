pub struct AbsorbedSource {
    pub id: &'static str,
    pub root: &'static str,
    pub remote: &'static str,
    pub branch: &'static str,
    pub snapshot_file: &'static str,
    pub lock_snapshot: &'static str,
    pub strip_manifest: &'static str,
    pub holdings: Option<&'static str>,
    pub sync: Sync,
}

pub enum Mechanism {
    CargoWorkspace,
    SingleCrate,
}

impl Mechanism {
    pub fn package_directory(self) -> PackageDirectory {
        match self {
            Mechanism::CargoWorkspace => PackageDirectory::None,
            Mechanism::SingleCrate => PackageDirectory::Required,
        }
    }

    pub fn placement(self) -> Placement {
        match self {
            Mechanism::CargoWorkspace => Placement::EveryMember,
            Mechanism::SingleCrate => Placement::DeclaredDirectory,
        }
    }
}

pub enum PackageDirectory {
    None,
    Required,
}

pub enum Placement {
    EveryMember,
    DeclaredDirectory,
}

pub struct Sync {
    pub mechanism: Mechanism,
    pub package_directory: Option<&'static str>,
    pub package_manifest: &'static str,
    pub build_config_files: &'static [&'static str],
    pub frontend: Option<Frontend>,
    pub keep: &'static [&'static str],
    pub mirror: &'static str,
}

impl Sync {
    pub fn in_pnpm_closure(&self) -> bool {
        self.frontend
            .as_ref()
            .map(|frontend| frontend.in_pnpm_closure().next().is_some())
            .unwrap_or(false)
    }
}

pub struct Frontend {
    pub roots: &'static [FrontendRoot],
}

impl Frontend {
    pub fn in_pnpm_closure(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.roots
            .iter()
            .filter(|root| matches!(root.membership, RootMembership::PnpmClosure))
            .map(|root| root.path.as_str())
    }

    pub fn payload(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.roots
            .iter()
            .filter(|root| matches!(root.membership, RootMembership::MiniAppPayload))
            .map(|root| root.path.as_str())
    }
}

pub enum RootMembership {
    PnpmClosure,
    MiniAppPayload,
}

pub struct FrontendRoot {
    pub path: FrontendRootPath,
    pub membership: RootMembership,
}

pub struct FrontendRootPath(&'static str);

impl FrontendRootPath {
    pub fn new(path: &'static str) -> Option<FrontendRootPath> {
        if path.starts_with('/') || path.split('/').any(|segment| segment == "..") {
            return None;
        }
        Some(FrontendRootPath(path))
    }

    pub fn as_str(&self) -> &'static str {
        self.0
    }
}
