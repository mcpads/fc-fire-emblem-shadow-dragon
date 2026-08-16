use std::collections::BTreeSet;

mod source_spec;
mod workspace;

pub(crate) use source_spec::{
    FRONT_END_RESULT_DIALOGUE_RECORD_IDS, RECORD_ACTION_COMPOSITE_STATE,
    RECORD_LIST_COMPOSITE_STATE, SAVE_SLOT_SELECTION_COMPOSITE_STATE, START_MENU_COMPOSITE_STATE,
};
pub(crate) use workspace::{
    FrontEndMenuPlan, extract_front_end_menu_workspace, plan_front_end_menu,
};

pub(crate) fn translated_fixed_string_indices() -> BTreeSet<u8> {
    source_spec::MENU_LABEL_SPECS
        .iter()
        .map(|spec| spec.index)
        .collect()
}
