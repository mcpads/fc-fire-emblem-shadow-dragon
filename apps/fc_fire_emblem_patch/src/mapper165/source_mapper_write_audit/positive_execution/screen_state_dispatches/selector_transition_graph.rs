use std::collections::{BTreeSet, VecDeque};

use anyhow::{Result, ensure};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StateTransition {
    pub(super) from: u8,
    pub(super) to: u8,
}

impl StateTransition {
    pub(super) const fn new(from: u8, to: u8) -> Self {
        Self { from, to }
    }
}

pub(super) fn reachable_selectors(
    role: &str,
    handler_domain: &BTreeSet<u8>,
    initial: impl IntoIterator<Item = u8>,
    transitions: impl IntoIterator<Item = StateTransition>,
) -> Result<BTreeSet<u8>> {
    let initial = initial.into_iter().collect::<BTreeSet<_>>();
    let transitions = transitions.into_iter().collect::<Vec<_>>();
    ensure!(
        !initial.is_empty() && initial.is_subset(handler_domain),
        "{role} entry escapes its handler table"
    );
    ensure!(
        transitions
            .iter()
            .all(|transition| handler_domain.contains(&transition.from)
                && handler_domain.contains(&transition.to)),
        "{role} transition escapes its handler table"
    );

    let mut reached = initial.clone();
    let mut pending = initial.into_iter().collect::<VecDeque<_>>();
    while let Some(selector) = pending.pop_front() {
        for target in transitions
            .iter()
            .filter_map(|transition| (transition.from == selector).then_some(transition.to))
        {
            if reached.insert(target) {
                pending.push_back(target);
            }
        }
    }
    Ok(reached)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closure_follows_only_edges_reached_from_the_bound_entry() {
        let handlers = BTreeSet::from([0, 1, 2, 3]);
        let transitions = [
            StateTransition::new(0, 1),
            StateTransition::new(1, 2),
            StateTransition::new(3, 2),
        ];

        assert_eq!(
            reachable_selectors("synthetic state machine", &handlers, [0], transitions).unwrap(),
            BTreeSet::from([0, 1, 2])
        );
    }

    #[test]
    fn closure_rejects_an_edge_beyond_the_owned_table() {
        let handlers = BTreeSet::from([0, 1]);
        assert!(
            reachable_selectors(
                "synthetic state machine",
                &handlers,
                [0],
                [StateTransition::new(1, 2)],
            )
            .is_err()
        );
    }
}
