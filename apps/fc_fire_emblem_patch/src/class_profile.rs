mod source;
mod workspace;

pub(crate) use source::bind_installed_consumers;
pub(crate) use workspace::{
    ClassProfilePlan, PROFILE_PAGE_SPLIT_INDEX, extract_class_profile_workspace,
    plan_class_profiles,
};
