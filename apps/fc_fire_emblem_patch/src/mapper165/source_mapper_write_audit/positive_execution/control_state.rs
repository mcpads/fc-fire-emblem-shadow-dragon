#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct PositiveControlState {
    pub(super) address: u16,
    pub(super) role: &'static str,
}

pub(super) const OUTER_SCREEN_STATE: u16 = 0x0024;
pub(super) const FIXED_SCHEDULER_STATE: u16 = 0x0025;
pub(super) const PRG_BANK_SHADOW: u16 = 0x0029;
pub(super) const MAIN_STATE: u16 = 0x0084;
pub(super) const TITLE_STATE: u16 = 0x057A;
pub(super) const TITLE_ANIMATION_STATE: u16 = 0x0587;
pub(super) const PENDING_SHARED_MENU_REQUEST_STATE: u16 = 0x05CC;
pub(super) const MAP_DIALOGUE_OUTER_STATE: u16 = 0x05DB;
pub(super) const SHARED_MENU_STATE: u16 = 0x05DE;
pub(super) const COMPOSITE_SCREEN_STATE: u16 = 0x05E8;
pub(super) const DIALOGUE_OR_SOUND_STATE: u16 = 0x05EE;

pub(super) const POSITIVE_CONTROL_STATES: [PositiveControlState; 11] = [
    PositiveControlState {
        address: OUTER_SCREEN_STATE,
        role: "outer_screen_state_24",
    },
    PositiveControlState {
        address: FIXED_SCHEDULER_STATE,
        role: "fixed_scheduler_state_25",
    },
    PositiveControlState {
        address: PRG_BANK_SHADOW,
        role: "prg_bank_shadow_29",
    },
    PositiveControlState {
        address: MAIN_STATE,
        role: "main_state_84",
    },
    PositiveControlState {
        address: TITLE_STATE,
        role: "title_state_057A",
    },
    PositiveControlState {
        address: TITLE_ANIMATION_STATE,
        role: "title_animation_state_0587",
    },
    PositiveControlState {
        address: PENDING_SHARED_MENU_REQUEST_STATE,
        role: "pending_shared_menu_request_state_05CC",
    },
    PositiveControlState {
        address: MAP_DIALOGUE_OUTER_STATE,
        role: "map_dialogue_outer_state_05DB",
    },
    PositiveControlState {
        address: SHARED_MENU_STATE,
        role: "shared_menu_state_05DE",
    },
    PositiveControlState {
        address: COMPOSITE_SCREEN_STATE,
        role: "composite_screen_state_05E8",
    },
    PositiveControlState {
        address: DIALOGUE_OR_SOUND_STATE,
        role: "dialogue_or_sound_state_05EE",
    },
];

pub(super) type ObservedControlStateWrites = BTreeMap<(u8, u16, u16), Option<BTreeSet<u8>>>;

pub(super) fn positive_control_state(address: u16) -> Option<PositiveControlState> {
    POSITIVE_CONTROL_STATES
        .iter()
        .copied()
        .find(|state| state.address == address)
}

pub(super) fn merge_observed_control_state_writes(
    merged: &mut ObservedControlStateWrites,
    observations: &ObservedControlStateWrites,
) {
    for (&site, values) in observations {
        merged
            .entry(site)
            .and_modify(|previous| {
                *previous = match (&*previous, values) {
                    (Some(previous), Some(values)) => {
                        Some(previous.union(values).copied().collect())
                    }
                    _ => None,
                };
            })
            .or_insert_with(|| values.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_positive_control_state_has_one_stable_role() {
        assert_eq!(
            POSITIVE_CONTROL_STATES
                .iter()
                .map(|state| state.address)
                .collect::<BTreeSet<_>>()
                .len(),
            POSITIVE_CONTROL_STATES.len()
        );
        assert!(
            POSITIVE_CONTROL_STATES
                .iter()
                .all(|state| !state.role.is_empty())
        );
    }

    #[test]
    fn one_unresolved_context_disqualifies_a_write_value_domain() {
        let site = (0x06, 0x8400, OUTER_SCREEN_STATE);
        let mut merged =
            ObservedControlStateWrites::from([(site, Some(BTreeSet::from([0x00, 0x01])))]);
        merge_observed_control_state_writes(
            &mut merged,
            &ObservedControlStateWrites::from([(site, None)]),
        );

        assert_eq!(merged.get(&site), Some(&None));
    }
}
use std::collections::{BTreeMap, BTreeSet};
