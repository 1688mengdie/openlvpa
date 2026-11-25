use crate::step::Step;

pub struct Node {
    pub step: Step,
    pub requires: &'static [Step],
}

pub const ORDER: [Node; 12] = [
    Node {
        step: Step::Wrapper,
        requires: &[],
    },
    Node {
        step: Step::Take,
        requires: &[Step::Wrapper],
    },
    Node {
        step: Step::Absorb,
        requires: &[Step::Take],
    },
    Node {
        step: Step::Merge,
        requires: &[Step::Absorb],
    },
    Node {
        step: Step::Strip,
        requires: &[Step::Merge],
    },
    Node {
        step: Step::Deshell,
        requires: &[Step::Strip],
    },
    Node {
        step: Step::Seam,
        requires: &[Step::Deshell],
    },
    Node {
        step: Step::Patches,
        requires: &[Step::Seam],
    },
    Node {
        step: Step::Install,
        requires: &[Step::Patches],
    },
    Node {
        step: Step::Frontend,
        requires: &[Step::Install],
    },
    Node {
        step: Step::Dev,
        requires: &[Step::Frontend],
    },
    Node {
        step: Step::Build,
        requires: &[Step::Frontend],
    },
];

pub enum Entrance {
    Dev,
    Build,
}

impl Entrance {
    pub fn terminus(self) -> Step {
        match self {
            Entrance::Dev => Step::Dev,
            Entrance::Build => Step::Build,
        }
    }
}

pub fn order_for(entrance: Entrance) -> Vec<Step> {
    let mut ordered = Vec::with_capacity(ORDER.len());
    let mut pending = Some(entrance.terminus());
    while let Some(step) = pending {
        let node = ORDER
            .iter()
            .find(|node| node.step == step)
            .unwrap_or_else(|| panic!("{step:?} names a step the chain does not carry"));
        ordered.push(node.step);
        pending = node.requires.first().copied();
    }
    ordered.reverse();
    ordered
}
