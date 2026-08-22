//! Supported-source dialogue state shared by inventory, generated code, and runtime evidence.
//!
//! Screen-specific modules may give these addresses role-specific aliases, but the numeric
//! source ABI lives here so analysis and production cannot drift independently.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MainDialogueRuntimeStateLayout {
    pub(crate) entry_index_address: u16,
    pub(crate) directory_selector_address: u16,
    pub(crate) state_address: u16,
    pub(crate) completion_flag_address: u16,
    pub(crate) caller_handoff_flag_address: u16,
    pub(crate) map_dialogue_outer_state_address: u16,
    pub(crate) map_dialogue_resume_state_address: u16,
    pub(crate) dialogue_or_sound_state_address: u16,
}

pub(crate) const MAIN_DIALOGUE_RUNTIME_STATE: MainDialogueRuntimeStateLayout =
    MainDialogueRuntimeStateLayout {
        entry_index_address: 0x77F1,
        directory_selector_address: 0x77F4,
        state_address: 0x77F7,
        completion_flag_address: 0x7803,
        caller_handoff_flag_address: 0x7809,
        map_dialogue_outer_state_address: 0x05DB,
        map_dialogue_resume_state_address: 0x05DC,
        dialogue_or_sound_state_address: 0x05EE,
    };

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogue_identity_and_lifecycle_addresses_do_not_overlap() {
        let layout = MAIN_DIALOGUE_RUNTIME_STATE;
        let addresses = [
            layout.entry_index_address,
            layout.directory_selector_address,
            layout.state_address,
            layout.completion_flag_address,
            layout.caller_handoff_flag_address,
            layout.map_dialogue_outer_state_address,
            layout.map_dialogue_resume_state_address,
            layout.dialogue_or_sound_state_address,
        ];
        assert_eq!(
            addresses
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            addresses.len()
        );
    }
}
