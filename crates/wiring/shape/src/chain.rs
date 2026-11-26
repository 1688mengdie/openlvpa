use std::sync::LazyLock;

use crate::step::Step;

pub struct Node {
    pub step: Step,
}

const STEPS: &str = include_str!("../../../../scripts/chain-order.mjs");

const ORDER_HEAD: &str = "const ORDER = [";

pub static ORDER: LazyLock<Vec<Node>> = LazyLock::new(|| {
    let named = STEPS
        .split_once(ORDER_HEAD)
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(body, _)| body)
        .unwrap_or_else(|| panic!("scripts/chain-order.mjs carries no `{ORDER_HEAD}` line this chain reads its step order from"));
    let mut nodes = Vec::new();
    for text in named.split(',') {
        let cleaned = text.trim();
        let name = cleaned
            .strip_prefix('\'')
            .and_then(|rest| rest.strip_suffix('\''))
            .map(|inner| inner.trim());
        let step = name
            .and_then(Step::from_name)
            .unwrap_or_else(|| panic!("`{cleaned}` stands in scripts/chain-order.mjs and names no step this chain carries"));
        if nodes.iter().any(|node: &Node| node.step == step) {
            panic!("`{cleaned}` stands twice in the step order scripts/chain-order.mjs carries");
        }
        nodes.push(Node { step });
    }
    nodes
});

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
    let terminus = entrance.terminus();
    let at = ORDER
        .iter()
        .position(|node| node.step == terminus)
        .unwrap_or_else(|| panic!("{terminus:?} names a step the chain does not carry"));
    ORDER[..=at].iter().map(|node| node.step).collect()
}
