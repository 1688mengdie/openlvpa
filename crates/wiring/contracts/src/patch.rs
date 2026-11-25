pub struct PatchFile {
    pub file: &'static str,
    pub replacements: &'static [Replacement],
}

pub struct Replacement {
    pub find: &'static str,
    pub replace: &'static str,
    pub anchor: Anchor,
}

pub enum Anchor {
    RelativeLocation,
    FileName,
    PublicContract,
    SemanticFunction,
}

pub struct AnchorRule {
    pub kind: Anchor,
    pub rejected: &'static [&'static str],
}

pub const ANCHORS: [AnchorRule; 4] = [
    AnchorRule {
        kind: Anchor::RelativeLocation,
        rejected: &["行号", "偏移", "字节", "具体实现", "内部结构"],
    },
    AnchorRule {
        kind: Anchor::FileName,
        rejected: &["行号", "偏移", "字节", "具体实现", "内部结构"],
    },
    AnchorRule {
        kind: Anchor::PublicContract,
        rejected: &["行号", "偏移", "字节", "具体实现", "内部结构"],
    },
    AnchorRule {
        kind: Anchor::SemanticFunction,
        rejected: &["行号", "偏移", "字节", "具体实现", "内部结构"],
    },
];

impl AnchorRule {
    pub fn admits(self, quantity: &str) -> bool {
        !self.rejected.contains(&quantity)
    }
}

pub enum PatchDefect {
    ZeroHits,
    AmbiguousHits,
    NoChange,
}

pub fn defect_of(hits: usize, changed: bool) -> Option<PatchDefect> {
    match (hits, changed) {
        (0, _) => Some(PatchDefect::ZeroHits),
        (1, false) => Some(PatchDefect::NoChange),
        (1, true) => None,
        _ => Some(PatchDefect::AmbiguousHits),
    }
}
