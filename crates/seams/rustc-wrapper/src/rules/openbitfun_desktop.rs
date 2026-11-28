use super::rename::Rename;
use super::{
    CrateSpec, Landing, Locate, Managed, Rule, Seam, SeamBody, SeamDependency, SeamHost, SeamSeed,
};

pub const SPEC: CrateSpec = CrateSpec {
    crate_path: "src/apps/desktop/src",
    source_tree: "obf",
    package: "openbitfun-desktop",
    crate_name: "openbitfun_desktop_lib",
    package_module: None,
    root: None,
    dependencies: HOST_DEPENDENCIES,
    seams: SEAMS,
    rules: RULES,
    carries: &[],
};

const HOST_DEPENDENCIES: &[SeamDependency] = &[SEAM_DEPENDENCY, SEAM_CORE];

const SEAM_CORE: SeamDependency = SeamDependency {
    name: "windows_core",
    package: "windows-core",
    version: Some("0.62"),
    path: "crates/seams/desktop-win32-thread",
    declared: "",
    features: &[],
    stands_down: false,
    landing: Landing::SeamModule,
    managed: Managed::Consolidated,
};

const SEAMS: &[Seam] = &[Seam {
    module: "lvpa_desktop_win32_thread",
    host: SeamHost::checked("crates/seams/desktop-win32-thread/src/lib.rs"),
    body: SeamBody::checked("crates/seams/desktop-win32-thread/src/lib.rs"),
    seed: SeamSeed::Module,
    seeds: &["lib.rs"],
    dependencies: &[SEAM_CORE],
}];

const SEAM_DEPENDENCY: SeamDependency = SeamDependency {
    name: "lvpa_desktop_win32_thread",
    package: "lvpa-desktop-win32-thread",
    version: None,
    path: "crates/seams/desktop-win32-thread",
    declared: "",
    features: &[],
    stands_down: false,
    landing: Landing::HostManifest,
    managed: Managed::Host,
};

const RULES: &[Rule] = &[Rule {
    id: "lvpa.desktop_layer.error_from_win32",
    file: "desktop_cards/desktop_layer.rs",
    hold_back_in: None,
    locate: Locate::Rename(Rename {
        from: "core::Error::from_win32()",
        to: "lvpa_desktop_win32_thread::error_from_win32()",
    }),
}];
