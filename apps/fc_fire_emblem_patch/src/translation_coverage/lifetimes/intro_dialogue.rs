use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{font_slots::ACTIVE_HANGUL_SLOT_COUNT, sha1_hex};

use super::super::{
    installed::InstalledIntroDialogueCapacity, report::TranslationLifetimeDemandReport,
};

pub(super) struct InputBindings<'a> {
    pub(super) capacities: &'a [InstalledIntroDialogueCapacity],
    pub(super) current_build_report_sha1: &'a str,
}

#[derive(Serialize)]
struct EvidenceDigest<'a> {
    schema: u8,
    screen_role: &'a str,
    current_build_report_sha1: &'a str,
    screen_evidence_manifest_sha1: &'a str,
    target_glyph_count: usize,
    preserved_active_code_count: usize,
    total_slot_demand: usize,
    scope: &'static str,
}

pub(super) fn inspect(bindings: InputBindings<'_>) -> Result<Vec<TranslationLifetimeDemandReport>> {
    ensure!(
        bindings.capacities.len() == 2,
        "installed intro-dialogue capacity count changed"
    );
    bindings
        .capacities
        .iter()
        .map(|capacity| {
            ensure!(
                ["intro_dialogue", "later_intro_dialogue"]
                    .contains(&capacity.screen_role)
                    && capacity.target_glyph_count + capacity.preserved_active_code_count
                        == capacity.total_slot_demand
                    && capacity.total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
                "{} installed capacity is incomplete",
                capacity.screen_role
            );
            let evidence = EvidenceDigest {
                schema: 1,
                screen_role: capacity.screen_role,
                current_build_report_sha1: bindings.current_build_report_sha1,
                screen_evidence_manifest_sha1: &capacity.screen_evidence_manifest_sha1,
                target_glyph_count: capacity.target_glyph_count,
                preserved_active_code_count: capacity.preserved_active_code_count,
                total_slot_demand: capacity.total_slot_demand,
                scope: "the installed page includes the exact chapter title and complete selected intro-dialogue chain; temporal source-screen samples exclude the dialogue interior while background and sprite pattern tables remain separate",
            };
            let evidence_bytes = serde_json::to_vec(&evidence)
                .context("serialize installed intro-dialogue lifetime evidence")?;
            Ok(TranslationLifetimeDemandReport {
                screen_role: capacity.screen_role,
                measurement_basis: "installed complete chapter-title and intro-dialogue page plus preserved source-screen and dialogue-chain active codes",
                target_glyph_count: capacity.target_glyph_count,
                preserved_active_source_code_count: capacity.preserved_active_code_count,
                additional_target_glyph_reservation_count: 0,
                total_slot_demand: capacity.total_slot_demand,
                active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
                fits_active_page: true,
                evidence_report_sha1: sha1_hex(&evidence_bytes),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_intro_pages_keep_early_and_later_roles_separate() {
        let capacities = [
            InstalledIntroDialogueCapacity {
                screen_role: "intro_dialogue",
                target_glyph_count: 82,
                preserved_active_code_count: 113,
                total_slot_demand: 195,
                screen_evidence_manifest_sha1: "chapter-one-evidence".to_owned(),
            },
            InstalledIntroDialogueCapacity {
                screen_role: "later_intro_dialogue",
                target_glyph_count: 58,
                preserved_active_code_count: 85,
                total_slot_demand: 143,
                screen_evidence_manifest_sha1: "chapter-two-evidence".to_owned(),
            },
        ];
        let demands = inspect(InputBindings {
            capacities: &capacities,
            current_build_report_sha1: "build-report",
        })
        .unwrap();

        assert_eq!(demands[0].screen_role, "intro_dialogue");
        assert_eq!(demands[0].total_slot_demand, 195);
        assert_eq!(demands[1].screen_role, "later_intro_dialogue");
        assert_eq!(demands[1].total_slot_demand, 143);
    }
}
