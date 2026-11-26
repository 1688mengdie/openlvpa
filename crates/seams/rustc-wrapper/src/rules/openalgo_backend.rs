use super::{CrateSpec, Locate, Rule, Seam, SeamBody, SeamHost, SeamSeed};

pub const SPEC: CrateSpec = CrateSpec {
    crate_path: "src",
    source_tree: "openalgo-desktop",
    package: "openalgo-desktop",
    crate_name: "openalgo_desktop_lib",
    package_module: None,
    dependencies: &[],
    seams: SEAMS,
    rules: BACKEND_RULES,
};

const SEAMS: &[Seam] = &[Seam {
    module: "lvpa_openalgo_backend",
    host: SeamHost::checked("crates/seams/openalgo-backend/src/lib.rs"),
    body: SeamBody::checked("crates/seams/openalgo-backend/src/lib.rs"),
    seed: SeamSeed::Module,
    seeds: &["lib.rs"],
    dependencies: &[],
}];

const BACKEND_RULES: &[Rule] = &[Rule {
    id: "lvpa.openalgo_backend.handler",
    file: "commands/mod.rs",
    locate: Locate::Composition(crate::rules::composition::Composition::Envelope {
        host: "crates/seams/openalgo-backend/src/lib.rs",
        envelope: "handler",
        seam: "lvpa_openalgo_backend",
        entry: "startup_status",
        site: "        lvpa_openalgo_backend::open(lvpa_openalgo_backend::Seam {\n            next: Box::new(handler),\n            refused: Default::default(),\n        }),\n",
    }),
}];
