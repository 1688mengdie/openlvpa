pub enum Table {
    Members,
    Exclude,
    WorkspaceDependencies,
    PatchesCratesIo,
    Profile,
}

impl Table {
    pub fn name(self) -> &'static str {
        match self {
            Table::Members => "members",
            Table::Exclude => "exclude",
            Table::WorkspaceDependencies => "workspace.dependencies",
            Table::PatchesCratesIo => "patch.crates-io",
            Table::Profile => "profile",
        }
    }

    pub fn top_level(self) -> bool {
        matches!(
            self,
            Table::WorkspaceDependencies | Table::PatchesCratesIo | Table::Profile
        )
    }
}

pub struct CargoGraph {
    pub root: &'static str,
    pub tables: &'static [Table],
    pub lock: &'static str,
}

pub fn cargo_graph() -> CargoGraph {
    CargoGraph {
        root: "Cargo.toml",
        tables: &[
            Table::Members,
            Table::Exclude,
            Table::WorkspaceDependencies,
            Table::PatchesCratesIo,
            Table::Profile,
        ],
        lock: "Cargo.lock",
    }
}

pub struct PnpmGraph {
    pub root: &'static str,
    pub lock: &'static str,
}

pub fn pnpm_graph() -> PnpmGraph {
    PnpmGraph {
        root: "pnpm-workspace.yaml",
        lock: "pnpm-lock.yaml",
    }
}
