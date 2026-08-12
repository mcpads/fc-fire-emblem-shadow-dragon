use std::{collections::BTreeSet, path::Path};

use anyhow::{Result, ensure};

use crate::{
    chapter_transition::{bind_save_complete_dialogue_records, plan_transition_labels},
    choice_labels::plan_choice_labels,
    dialogue_assets::plan_main_dialogue_bundle,
    rom::Rom,
    suspend_message::bind_suspend_message_to_main_dialogue,
};

use super::super::report::TranslationLifetimeDemandReport;

mod demand;
mod runtime_evidence;

use demand::EvidenceBindings;

pub(super) struct InputBindings<'a> {
    pub(super) source_path: &'a Path,
    pub(super) main_dialogue_workspace_path: &'a Path,
    pub(super) choice_label_workspace_path: &'a Path,
    pub(super) transition_label_workspace_path: &'a Path,
    pub(super) continue_prompt_manifest_path: &'a Path,
    pub(super) main_dialogue_workspace_sha1: &'a str,
    pub(super) choice_label_workspace_sha1: &'a str,
    pub(super) transition_label_workspace_sha1: &'a str,
}

pub(super) fn inspect(bindings: InputBindings<'_>) -> Result<Vec<TranslationLifetimeDemandReport>> {
    let rom = Rom::from_path(bindings.source_path)?;
    rom.verify_supported_japanese()?;
    let transition = plan_transition_labels(&rom, bindings.transition_label_workspace_path)?;
    let choices = plan_choice_labels(&rom, bindings.choice_label_workspace_path)?;
    ensure!(
        transition.save_offer.workspace_sha1 == bindings.transition_label_workspace_sha1
            && choices.workspace_sha1 == bindings.choice_label_workspace_sha1,
        "chapter-save lifetime localization input changed"
    );

    let selected_records = bind_save_complete_dialogue_records(&rom)?;
    let continue_prompt = plan_main_dialogue_bundle(
        &rom,
        bindings.main_dialogue_workspace_path,
        &[selected_records.continue_prompt],
    )?;
    let power_off_notice = plan_main_dialogue_bundle(
        &rom,
        bindings.main_dialogue_workspace_path,
        &[selected_records.power_off_notice],
    )?;
    ensure!(
        continue_prompt.workspace_sha1 == bindings.main_dialogue_workspace_sha1
            && power_off_notice.workspace_sha1 == bindings.main_dialogue_workspace_sha1,
        "chapter-save lifetime main-dialogue input changed"
    );
    bind_suspend_message_to_main_dialogue(&rom)?;

    let choice_glyphs = choices.unique_glyphs();
    let save_offer_target_glyphs =
        union_chars(&transition.save_offer.target_glyphs, &choice_glyphs);
    let save_offer_source_codes = union_codes(
        &transition.save_offer.source_reclaimable_active_codes,
        &choices.source_reclaimable_active_codes,
    );
    let continue_prompt_target_glyphs =
        union_chars(&continue_prompt.unique_glyphs(), &choice_glyphs);
    let continue_prompt_runtime = runtime_evidence::load(bindings.continue_prompt_manifest_path)?;
    let active_codes = crate::font_slots::active_hangul_codes()
        .into_iter()
        .collect::<BTreeSet<_>>();
    let mut continue_prompt_preserved_codes = continue_prompt_runtime
        .preserved_screen_active_codes
        .clone();
    continue_prompt_preserved_codes.extend(
        continue_prompt
            .preserved_source_codes
            .intersection(&active_codes)
            .copied(),
    );
    continue_prompt_preserved_codes.extend(&choices.preserved_active_codes);

    Ok(vec![
        demand::full_page(
            "chapter_save_offer",
            "full-page upper bound for the exact save-question and yes/no label consumers",
            &save_offer_target_glyphs,
            &save_offer_source_codes,
            EvidenceBindings {
                main_dialogue_workspace_sha1: bindings.main_dialogue_workspace_sha1,
                choice_label_workspace_sha1: Some(bindings.choice_label_workspace_sha1),
                transition_label_workspace_sha1: Some(bindings.transition_label_workspace_sha1),
                runtime_manifest_sha1: None,
                main_dialogue_record_id: None,
                source_binding: "source-bound fixed-label composer and shared yes/no consumers",
            },
        )?,
        demand::observed_screen(
            "chapter_save_complete_continue_prompt",
            "irregular frozen-frame union outside the exact B0:00 dialogue and yes/no target cells",
            &continue_prompt_target_glyphs,
            &continue_prompt_preserved_codes,
            EvidenceBindings {
                main_dialogue_workspace_sha1: bindings.main_dialogue_workspace_sha1,
                choice_label_workspace_sha1: Some(bindings.choice_label_workspace_sha1),
                transition_label_workspace_sha1: None,
                runtime_manifest_sha1: Some(&continue_prompt_runtime.manifest_sha1),
                main_dialogue_record_id: Some(selected_records.continue_prompt),
                source_binding: "source-bound save-complete handler selects B0:00",
            },
        )?,
        demand::full_page(
            "chapter_save_complete_power_off_notice",
            "full-page upper bound for the exact B0:01 terminal-notice consumer",
            &power_off_notice.unique_glyphs(),
            &power_off_notice.source_reclaimable_active_codes,
            EvidenceBindings {
                main_dialogue_workspace_sha1: bindings.main_dialogue_workspace_sha1,
                choice_label_workspace_sha1: None,
                transition_label_workspace_sha1: None,
                runtime_manifest_sha1: None,
                main_dialogue_record_id: Some(selected_records.power_off_notice),
                source_binding: "source-bound save-complete handler selects B0:01",
            },
        )?,
        demand::full_page(
            "suspend_message",
            "full-page upper bound for the exact B0:01 suspend-message consumer",
            &power_off_notice.unique_glyphs(),
            &power_off_notice.source_reclaimable_active_codes,
            EvidenceBindings {
                main_dialogue_workspace_sha1: bindings.main_dialogue_workspace_sha1,
                choice_label_workspace_sha1: None,
                transition_label_workspace_sha1: None,
                runtime_manifest_sha1: None,
                main_dialogue_record_id: Some(selected_records.power_off_notice),
                source_binding: "source-bound suspend handler selects B0:01",
            },
        )?,
    ])
}

fn union_chars(left: &BTreeSet<char>, right: &BTreeSet<char>) -> BTreeSet<char> {
    left.union(right).copied().collect()
}

fn union_codes(left: &BTreeSet<u8>, right: &BTreeSet<u8>) -> BTreeSet<u8> {
    left.union(right).copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_save_and_suspend_consumers_fit_the_active_page() {
        let project_root = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        let source =
            project_root.join("roms/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan).nes");
        if !source.exists() {
            return;
        }
        let main_dialogue = project_root.join("private/dialogue/main-workspace.json");
        let choices = project_root.join("assets/translation/choice-labels.ko.json");
        let transition = project_root.join("assets/translation/transition-labels.ko.json");
        let continue_prompt_manifest = project_root
            .join("evidence/private/chapter-save-lifetimes/continue-prompt-manifest.json");
        let demands = inspect(InputBindings {
            source_path: &source,
            main_dialogue_workspace_path: &main_dialogue,
            choice_label_workspace_path: &choices,
            transition_label_workspace_path: &transition,
            continue_prompt_manifest_path: &continue_prompt_manifest,
            main_dialogue_workspace_sha1: &crate::sha1_hex(&std::fs::read(&main_dialogue).unwrap()),
            choice_label_workspace_sha1: &crate::sha1_hex(&std::fs::read(&choices).unwrap()),
            transition_label_workspace_sha1: &crate::sha1_hex(&std::fs::read(&transition).unwrap()),
        })
        .unwrap();

        // 화면 역할과 그 순서는 원본 구조라 고정한다.
        assert_eq!(
            demands
                .iter()
                .map(|demand| demand.screen_role)
                .collect::<Vec<_>>(),
            [
                "chapter_save_offer",
                "chapter_save_complete_continue_prompt",
                "chapter_save_complete_power_off_notice",
                "suspend_message",
            ]
        );
        // 슬롯 수요는 확정한 번역에 따라 달라지므로 고정하지 않는다.
        // 지켜야 하는 것은 네 소비자가 모두 활성 페이지 예산 안에 든다는 점이다.
        assert!(demands.iter().all(|demand| demand.fits_active_page));
        assert!(
            demands
                .iter()
                .all(|demand| demand.total_slot_demand <= demand.active_slot_count)
        );
    }
}
