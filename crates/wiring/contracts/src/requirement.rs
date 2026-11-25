use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Requirement {
    MirrorWorkspaceSingle,
    UpstreamSourceOnlyByMergeThenStrip,
    SeamCratePlacement,
    WiringCratePlacement,
    ExtensionMechanismOnly,
    DevChainFromRepoRoot,
    BuildChainFromRepoRoot,
    UpstreamDeclarationsMatchNotice,
    MiniAppFrontendWithoutShell,
    EveryArtefactAnswersItsNeed,
    EveryClauseHoldOnDisk,
}

impl Requirement {
    pub const ALL: [Requirement; 11] = [
        Requirement::MirrorWorkspaceSingle,
        Requirement::UpstreamSourceOnlyByMergeThenStrip,
        Requirement::SeamCratePlacement,
        Requirement::WiringCratePlacement,
        Requirement::ExtensionMechanismOnly,
        Requirement::DevChainFromRepoRoot,
        Requirement::BuildChainFromRepoRoot,
        Requirement::UpstreamDeclarationsMatchNotice,
        Requirement::MiniAppFrontendWithoutShell,
        Requirement::EveryArtefactAnswersItsNeed,
        Requirement::EveryClauseHoldOnDisk,
    ];
}

impl fmt::Display for Requirement {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Requirement::MirrorWorkspaceSingle => "MirrorWorkspaceSingle",
            Requirement::UpstreamSourceOnlyByMergeThenStrip => {
                "UpstreamSourceOnlyByMergeThenStrip"
            }
            Requirement::SeamCratePlacement => "SeamCratePlacement",
            Requirement::WiringCratePlacement => "WiringCratePlacement",
            Requirement::ExtensionMechanismOnly => "ExtensionMechanismOnly",
            Requirement::DevChainFromRepoRoot => "DevChainFromRepoRoot",
            Requirement::BuildChainFromRepoRoot => "BuildChainFromRepoRoot",
            Requirement::UpstreamDeclarationsMatchNotice => {
                "UpstreamDeclarationsMatchNotice"
            }
            Requirement::MiniAppFrontendWithoutShell => "MiniAppFrontendWithoutShell",
            Requirement::EveryArtefactAnswersItsNeed => "EveryArtefactAnswersItsNeed",
            Requirement::EveryClauseHoldOnDisk => "EveryClauseHoldOnDisk",
        })
    }
}

pub struct Clause {
    pub requirement: Requirement,
    pub statement: &'static str,
    pub judged_at: &'static [&'static str],
}

pub const CLAUSES: [Clause; 11] = [
    Clause {
        requirement: Requirement::MirrorWorkspaceSingle,
        statement: "仓根 Cargo.toml 的 members 收全两棵吸收树内的每个 crate，且这些 crate 只见于这一份成员清单：吸收树内无 [workspace] 表；两棵树的依赖与补丁各合成仓根 [workspace.dependencies] 与 [patch.crates-io] 的同一份生成区；锁只有仓根 Cargo.lock 一份，吸收树内无 Cargo.lock、pnpm-workspace.yaml、pnpm-lock.yaml、package-lock.json、yarn.lock、bun.lock、bun.lockb、npm-shrinkwrap.json 中任一件。",
        judged_at: &[
            "Cargo.toml",
            "Cargo.lock",
            "pnpm-workspace.yaml",
            "pnpm-lock.yaml",
            "scripts/absorbed-sources.json",
        ],
    },
    Clause {
        requirement: Requirement::UpstreamSourceOnlyByMergeThenStrip,
        statement: "两棵吸收树按 scripts/strip-manifest.json 的排除集取入，落进仓根的是成员、依赖、补丁边与锁解析结果而非树内原件，且树内每一份成员、依赖、版本在仓根都指得到；上游源码的改动以 patches/ 下由同步步套用的补丁形式存在，不改动吸收树内的字节；补丁改变的每个 crate 边出现在仓根 [patch.crates-io] 的生成区；补丁锚点只落在仓内相对位置、文件名、对外契约、内部语义函数四类上。",
        judged_at: &[
            "scripts/strip-manifest.json",
            "patches/",
            "Cargo.toml",
            "scripts/sync-upstream.mjs",
        ],
    },
    Clause {
        requirement: Requirement::SeamCratePlacement,
        statement: "我方接缝件位于 crates/seams/<名>/，经仓根 members 作同一镜像 workspace 的普通成员，并经树内生成的路径边接进被接的树内件；接缝锚点只落在仓内相对位置、文件名、对外契约、内部语义函数四类上，未锚行号、偏移、字节、具体实现或内部结构，且接缝在任意上游更新后仍可解析。",
        judged_at: &["crates/seams/", "Cargo.toml"],
    },
    Clause {
        requirement: Requirement::WiringCratePlacement,
        statement: "本链自身的编排件位于 crates/wiring/ 之下，并作镜像 workspace 成员由仓根统一编译。",
        judged_at: &["crates/wiring/", "Cargo.toml"],
    },
    Clause {
        requirement: Requirement::ExtensionMechanismOnly,
        statement: "wrapper 与 mod、插件等其余扩展机制在盘上落位并作扩展骨架，我方对后端的每一处改动都由这套机制承载，在编译期与静态源码一起合成进镜像 workspace；obf 多 crate 后端取用其成熟面，openalgo-desktop 单 crate 后端经同一套机制接入 obf 后端架构，吸收树内后端源码无一处我方改动。",
        judged_at: &[
            "crates/seams/",
            "crates/wiring/",
            "patches/",
            "Cargo.toml",
            "scripts/absorbed-sources.json",
        ],
    },
    Clause {
        requirement: Requirement::DevChainFromRepoRoot,
        statement: "在仓根执行 pnpm run desktop:dev 起得来、退得干净，且改动我方任一处源码即生效。",
        judged_at: &["package.json", "scripts/dev.mjs"],
    },
    Clause {
        requirement: Requirement::BuildChainFromRepoRoot,
        statement: "在仓根执行 pnpm run desktop:build 出得一件可安装成品。",
        judged_at: &["package.json", "scripts/desktop-build.mjs"],
    },
    Clause {
        requirement: Requirement::UpstreamDeclarationsMatchNotice,
        statement: "scripts/absorbed-sources.json 为两棵吸收树各声明的根目录、远端与分支，与 NOTICE 所列上游的远端与分支逐字一致。",
        judged_at: &["NOTICE", "scripts/absorbed-sources.json"],
    },
    Clause {
        requirement: Requirement::MiniAppFrontendWithoutShell,
        statement: "openalgo-desktop 前端去壳后落 MiniApp 用户配置位并带 i18n 中文补丁显示，该补丁在 patches/ 内由同步步套用；其 Tauri 壳 src-tauri 不保留、不出现在仓根 members、不在 pnpm-workspace.yaml 的成员闭包，其后端 crate 作扩展介入 obf 后端并出现在仓根 members；原生前端壳只有 obf 一处。",
        judged_at: &[
            "patches/",
            "Cargo.toml",
            "pnpm-workspace.yaml",
            "scripts/absorbed-sources.json",
        ],
    },
    Clause {
        requirement: Requirement::EveryArtefactAnswersItsNeed,
        statement: "盘上每件入骨架的产物都答得出它服务哪一段链、哪一条需求，答不出的不入骨架。",
        judged_at: &["Cargo.toml", "pnpm-workspace.yaml", "crates/", "scripts/"],
    },
    Clause {
        requirement: Requirement::EveryClauseHoldOnDisk,
        statement: "CLAUSES 第 1～10 条每条都在其 judged_at 所列路径上成立、指得出在哪一处。",
        judged_at: &["crates/wiring/contracts/src/requirement.rs"],
    },
];

pub fn clause(requirement: Requirement) -> &'static Clause {
    CLAUSES
        .iter()
        .find(|entry| entry.requirement == requirement)
        .unwrap_or_else(|| unreachable!("every requirement has a clause"))
}
