use super::completion::Completion;
use super::{
    CrateSpec, Landing, Locate, Managed, Rule, Seam, SeamBody, SeamDependency, SeamHost, SeamSeed,
};

pub const SPEC: CrateSpec = CrateSpec {
    crate_path: "src/crates/services/computer-use/src",
    source_tree: "obf",
    package: "openbitfun-computer-use",
    crate_name: "openbitfun_computer_use",
    package_module: None,
    root: None,
    dependencies: HOST_DEPENDENCIES,
    seams: SEAMS,
    rules: RULES,
    carries: &[],
};

const SEAMS: &[Seam] = &[Seam {
    module: "lvpa_windows_future",
    host: SeamHost::checked("crates/seams/windows-future/src/lib.rs"),
    body: SeamBody::checked("crates/seams/windows-future/src/lib.rs"),
    seed: SeamSeed::Imports,
    seeds: &["lib.rs"],
    dependencies: &[],
}];

const HOST_DEPENDENCIES: &[SeamDependency] = &[SEAM_DEPENDENCY];

const SEAM_DEPENDENCY: SeamDependency = SeamDependency {
    name: "lvpa_windows_future",
    package: "lvpa-windows-future",
    version: None,
    path: "crates/seams/windows-future",
    declared: "",
    features: &[],
    stands_down: false,
    landing: Landing::HostManifest,
    managed: Managed::Host,
};

const RULES: &[Rule] = &[
    Rule {
        id: "lvpa.windows_future.screen_ocr_store",
        file: "computer_use/screen_ocr.rs",
        hold_back_in: None,
        locate: Locate::Completion(COMPLETION),
    },
    Rule {
        id: "lvpa.windows_future.screen_ocr_flush",
        file: "computer_use/screen_ocr.rs",
        hold_back_in: None,
        locate: Locate::Completion(COMPLETION),
    },
    Rule {
        id: "lvpa.windows_future.screen_ocr_decoder",
        file: "computer_use/screen_ocr.rs",
        hold_back_in: None,
        locate: Locate::Completion(COMPLETION),
    },
    Rule {
        id: "lvpa.windows_future.screen_ocr_software_bitmap",
        file: "computer_use/screen_ocr.rs",
        hold_back_in: None,
        locate: Locate::Completion(COMPLETION),
    },
    Rule {
        id: "lvpa.windows_future.screen_ocr_recognize",
        file: "computer_use/screen_ocr.rs",
        hold_back_in: None,
        locate: Locate::Completion(COMPLETION),
    },
];

const COMPLETION: Completion = Completion {
    seam: "lvpa_windows_future",
    exports: "crates/seams/windows-future/src/lib.rs",
    module: "get",
    trait_name: "Completion",
    method: "value",
    source: "IAsyncOperation",
    returns: "join",
};
