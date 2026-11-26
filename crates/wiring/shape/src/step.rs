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

    pub fn intent(self) -> &'static str {
        match self {
            Step::Wrapper => "扩展骨架落位，此后每一次编译都经 wrapper 与其余扩展机制发声",
            Step::Take => "两棵吸收树按 NOTICE 所列远端与分支取入，先合进仓根一套再删净树内能自行发声的原件",
            Step::Absorb => "两棵树的成员、依赖、补丁边与锁解析结果合成同一镜像仓 workspace 的一份成员清单、一套依赖、一把锁",
            Step::Merge => "树内 workspace 与包管理声明并入仓根生成区，树内不再存第二种说法",
            Step::Strip => "吸收树内不留任何能自行发声的一件，只留静态源码",
            Step::Deshell => "把不作成员的壳面从成员目录里摘去，后断面留作成员",
            Step::Seam => "我方接缝件按 crates/seams/<名>/ 落位并作镜像仓 workspace 的普通成员接进所接的树内件",
            Step::Patches => "我方补丁按锚点套用到树内原件，改后的静态源码成入镜像仓 workspace",
            Step::Install => "全部依赖产物与生成物只落仓根与用户配置目录，随后在镜像仓 workspace 里发生唯一一次编译与安装",
            Step::Frontend => "obf 原生前端作唯一外壳、openalgo-desktop 前端去壳后带 i18n 中文补丁落 MiniApp 用户配置位",
            Step::Dev => "开发模式跑完整条链，改我方任一处源码即生效，退得干净",
            Step::Build => "出包模式跑完整条链，交出一件可安装成品",
        }
    }

    pub fn guarantee(self) -> Guarantee {
        match self {
            Step::Wrapper => Guarantee {
                on_disk: &["crates/wiring/", "patches/"],
                leaves: &["target:lvpa-rustc-wrapper", "target:lvpa-capability-panorama"],
            },
            Step::Take => Guarantee {
                on_disk: &[
                    "upstream:obf",
                    "upstream:openalgo-desktop",
                    "Cargo.toml",
                    "Cargo.lock",
                    "pnpm-workspace.yaml",
                    "pnpm-lock.yaml",
                ],
                leaves: &["tree:obf", "tree:openalgo-desktop"],
            },
            Step::Absorb => Guarantee {
                on_disk: &[
                    "Cargo.toml",
                    "Cargo.lock",
                    "pnpm-workspace.yaml",
                    "pnpm-lock.yaml",
                ],
                leaves: &[
                    "Cargo.toml",
                    "Cargo.lock",
                    "pnpm-workspace.yaml",
                    "pnpm-lock.yaml",
                ],
            },
            Step::Merge => Guarantee {
                on_disk: &["Cargo.toml", "pnpm-workspace.yaml", ".gitattributes"],
                leaves: &["Cargo.toml", "pnpm-workspace.yaml", ".gitattributes"],
            },
            Step::Strip => Guarantee {
                on_disk: &["upstream:obf", "upstream:openalgo-desktop"],
                leaves: &["tree:obf", "tree:openalgo-desktop"],
            },
            Step::Deshell => Guarantee {
                on_disk: &["upstream:obf", "upstream:openalgo-desktop"],
                leaves: &["member:openalgo-desktop"],
            },
            Step::Seam => Guarantee {
                on_disk: &["crates/seams/", "Cargo.toml"],
                leaves: &["Cargo.toml"],
            },
            Step::Patches => Guarantee {
                on_disk: &["patches/", "Cargo.toml"],
                leaves: &["Cargo.toml"],
            },
            Step::Install => Guarantee {
                on_disk: &["Cargo.toml", "Cargo.lock", "pnpm-workspace.yaml", "pnpm-lock.yaml"],
                leaves: &["target:lvpa-build-bench", "target:pnpm-store"],
            },
            Step::Frontend => Guarantee {
                on_disk: &["pnpm-workspace.yaml", "pnpm-lock.yaml", "patches/", "target:lvpa-build-bench"],
                leaves: &["miniapp:"],
            },
            Step::Dev => Guarantee {
                on_disk: &["crates/"],
                leaves: &["root:.openlvpa/"],
            },
            Step::Build => Guarantee {
                on_disk: &["crates/"],
                leaves: &["root:.openlvpa/", "target:release/bundle"],
            },
        }
    }
}

pub struct Guarantee {
    pub on_disk: &'static [&'static str],
    pub leaves: &'static [&'static str],
}
