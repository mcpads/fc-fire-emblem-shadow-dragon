mod source_spec;
mod workspace;

pub(crate) use source_spec::{
    POINTER_LOAD_ADDRESS, POINTER_LOAD_BYTES, SOURCE_PRG_BANK as CHOICE_LABEL_SOURCE_PRG_BANK,
};
pub(crate) use workspace::{ChoiceLabelPlan, plan_choice_labels};
