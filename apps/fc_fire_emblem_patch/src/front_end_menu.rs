mod source_spec;
mod workspace;

pub(crate) use source_spec::SAVE_SLOT_SELECTION_COMPOSITE_STATE;
pub(crate) use workspace::{
    FrontEndMenuPlan, extract_front_end_menu_workspace, plan_front_end_menu,
};
