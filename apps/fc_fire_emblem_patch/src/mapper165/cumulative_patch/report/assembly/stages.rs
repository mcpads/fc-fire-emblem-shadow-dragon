use super::*;

pub(super) fn stage_reports(inputs: &CumulativeReportInputs<'_>) -> Vec<CumulativeStageReport> {
    let ui_stage = inputs.ui_stage;
    let chapter_one_output_sha1 = inputs.chapter_one_output_sha1.clone();
    let chapter_two_output_sha1 = inputs.chapter_two_output_sha1.clone();
    let front_end_stage = inputs.front_end_stage;
    let unit_name_stage = inputs.unit_name_stage;
    let class_profile_stage = inputs.class_profile_stage;
    let shop_dialogue_stage = inputs.shop_dialogue_stage;
    let weapon_shop_shared_text_stage = inputs.weapon_shop_shared_text_stage;
    let battle_stage = inputs.battle_stage;
    let maximum_dialogue_stage = inputs.maximum_dialogue_stage;
    let title_logo_stage = inputs.title_logo_stage;

    vec![
        CumulativeStageReport {
            role: "mapper165_options_and_roster",
            output_sha1: ui_stage.output_sha1.clone(),
            report_sha1: Some(ui_stage.report_sha1.clone()),
        },
        CumulativeStageReport {
            role: "chapter_1_intro_title_and_dialogue_transition_chain",
            output_sha1: chapter_one_output_sha1,
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "chapter_2_intro_title_and_dialogue",
            output_sha1: chapter_two_output_sha1,
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "front_end_menu",
            output_sha1: front_end_stage.output_sha1.clone(),
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "playable_unit_names_for_roster_and_unit_ui",
            output_sha1: unit_name_stage.output_sha1.clone(),
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "automatic_class_profile_titles_and_descriptions",
            output_sha1: class_profile_stage.output_sha1.clone(),
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "weapon_shop_dialogue_branches",
            output_sha1: shop_dialogue_stage.output_sha1.clone(),
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "weapon_shop_shared_item_names_and_choice_labels",
            output_sha1: weapon_shop_shared_text_stage.output_sha1.clone(),
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "battle_text_and_dynamic_composition",
            output_sha1: battle_stage.output_sha1.clone(),
            report_sha1: Some(battle_stage.loader_report_sha1.clone()),
        },
        CumulativeStageReport {
            role: "chapter_7_maximum_dialogue_page_reload",
            output_sha1: maximum_dialogue_stage.output_sha1.clone(),
            report_sha1: None,
        },
        CumulativeStageReport {
            role: "source_bound_korean_title_logo",
            output_sha1: title_logo_stage.output_sha1.clone(),
            report_sha1: None,
        },
    ]
}
