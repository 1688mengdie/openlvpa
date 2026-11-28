#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Wrapper,
    Take,
    Absorb,
    Merge,
    Strip,
    Deshell,
    Seam,
    Patches,
    Install,
    Frontend,
    Dev,
    Build,
}

pub struct Guarantee {
    pub on_disk: &'static [&'static str],
    pub leaves: &'static [&'static str],
}

impl Step {
    pub fn name(self) -> &'static str {
        match self {
            Step::Wrapper => "wrapper",
            Step::Take => "take",
            Step::Absorb => "absorb",
            Step::Merge => "merge",
            Step::Strip => "strip",
            Step::Deshell => "deshell",
            Step::Seam => "seam",
            Step::Patches => "patches",
            Step::Install => "install",
            Step::Frontend => "frontend",
            Step::Dev => "dev",
            Step::Build => "build",
        }
    }

    pub fn guarantee(self) -> Guarantee {
        match self {
            Step::Wrapper => Guarantee {
                on_disk: &["rustc-wrapper-build-dir"],
                leaves: &["rustc-wrapper", "capability-panorama", "rustc-wrapper-build-dir"],
            },
            Step::Take => Guarantee {
                on_disk: &["obf-tree", "openalgo-desktop-tree"],
                leaves: &["obf-tree", "openalgo-desktop-tree"],
            },
            Step::Absorb => Guarantee {
                on_disk: &[],
                leaves: &["host-notice"],
            },
            Step::Merge => Guarantee {
                on_disk: &["host-manifest", "host-pnpm-workspace", "host-gitattributes"],
                leaves: &["host-manifest", "host-lock", "host-pnpm-workspace", "host-pnpm-lock", "host-gitattributes"],
            },
            Step::Strip => Guarantee {
                on_disk: &["obf-tree", "openalgo-desktop-tree"],
                leaves: &["obf-tree", "openalgo-desktop-tree"],
            },
            Step::Deshell => Guarantee {
                on_disk: &["obf-tree", "openalgo-desktop-tree"],
                leaves: &["openalgo-desktop-backend"],
            },
            Step::Seam => Guarantee {
                on_disk: &["seam-crates", "host-manifest"],
                leaves: &["host-manifest", "seam-run-record"],
            },
            Step::Patches => Guarantee {
                on_disk: &["patch-declarations", "host-manifest"],
                leaves: &["host-manifest"],
            },
            Step::Install => Guarantee {
                on_disk: &[
                    "host-manifest",
                    "host-lock",
                    "host-pnpm-workspace",
                    "host-pnpm-lock",
                    "trees-carry-no-artifact",
                ],
                leaves: &["build-bench", "pnpm-store"],
            },
            Step::Frontend => Guarantee {
                on_disk: &[
                    "host-pnpm-workspace",
                    "host-pnpm-lock",
                    "patch-declarations",
                    "build-bench",
                ],
                leaves: &["miniapp-payload"],
            },
            Step::Dev => Guarantee {
                on_disk: &["own-crates", "trees-carry-no-artifact"],
                leaves: &["work-dir"],
            },
            Step::Build => Guarantee {
                on_disk: &["own-crates", "trees-carry-no-artifact"],
                leaves: &["work-dir", "bundle"],
            },
        }
    }
}

pub static ORDER: &[Step] = &[
    Step::Wrapper,
    Step::Take,
    Step::Absorb,
    Step::Merge,
    Step::Strip,
    Step::Deshell,
    Step::Seam,
    Step::Patches,
    Step::Install,
    Step::Frontend,
    Step::Dev,
    Step::Build,
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Entrance {
    Dev,
    Build,
}

impl Entrance {
    pub fn name(self) -> &'static str {
        match self {
            Entrance::Dev => "dev",
            Entrance::Build => "build",
        }
    }

    pub fn terminus(self) -> Step {
        match self {
            Entrance::Dev => Step::Dev,
            Entrance::Build => Step::Build,
        }
    }
}
