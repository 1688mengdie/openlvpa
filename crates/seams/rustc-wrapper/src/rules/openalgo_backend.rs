use super::{
    CrateSpec, Landing, Locate, Managed, Rule, Seam, SeamBody, SeamDependency, SeamHost, SeamSeed,
};

pub const SPEC: CrateSpec = CrateSpec {
    crate_path: "src-tauri",
    source_tree: "openalgo-desktop",
    package: "openalgo-desktop",
    crate_name: "openalgo_desktop_lib",
    package_module: None,
    // the take strips the tree's own `src-tauri` and the mirror keeps the crate's sources, so the
    // crate root the mirror compiles stands where the backend crate's source directory is
    root: Some("src"),
    dependencies: HOST_DEPENDENCIES,
    seams: SEAMS,
    rules: BACKEND_RULES,
    carries: &[],
};

const MEMBER_EDGE: SeamDependency = SeamDependency {
    name: "libsqlite3-sys",
    package: "libsqlite3-sys",
    version: Some("0.39"),
    path: "",
    declared: "libsqlite3-sys",
    features: &[],
    stands_down: false,
    landing: Landing::MemberManifest,
    managed: Managed::Replaced,
};

const SEAM_EDGE: SeamDependency = SeamDependency {
    name: "lvpa_openalgo_backend",
    package: "lvpa-seam-openalgo-backend",
    version: None,
    path: "crates/seams/openalgo-backend",
    declared: "",
    features: &[],
    stands_down: false,
    landing: Landing::HostManifest,
    managed: Managed::Host,
};

const HOST_DEPENDENCIES: &[SeamDependency] = &[MEMBER_EDGE, SEAM_EDGE];

const SEAMS: &[Seam] = &[Seam {
    module: "lvpa_openalgo_backend",
    host: SeamHost::checked("crates/seams/openalgo-backend/src/lib.rs"),
    body: SeamBody::checked("crates/seams/openalgo-backend/src/lib.rs"),
    seed: SeamSeed::Module,
    seeds: &["lib.rs"],
    dependencies: &[SEAM_EDGE],
}];

const BACKEND_RULES: &[Rule] = &[Rule {
    id: "lvpa.openalgo_backend.handler",
    file: "commands/mod.rs",
    hold_back_in: None,
    locate: Locate::Composition(crate::rules::composition::Composition::Envelope {
        host: "crates/seams/openalgo-backend/src/hand_off.rs",
        envelope: "handler",
        seam: "lvpa_openalgo_backend",
        entry: "window_proof",
    }),
}];
