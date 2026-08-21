use anyhow::{Context, Result, ensure};

use crate::{
    font_slots::ACTIVE_HANGUL_SLOT_COUNT,
    translation_coverage::{
        installed::InstalledChoiceLifetime, report::TranslationLifetimeDemandReport,
        screen_targets::STORAGE_FOLLOW_UP_CHOICE_SCREEN_ROLE,
    },
};

pub(super) fn inspect(
    installed: &InstalledChoiceLifetime,
) -> Result<TranslationLifetimeDemandReport> {
    let demand = &installed.storage_follow_up;
    let component_total = demand
        .target_glyph_count
        .checked_add(demand.preserved_active_code_count)
        .context("storage follow-up choice slot demand overflow")?;
    ensure!(
        installed.screen_role == STORAGE_FOLLOW_UP_CHOICE_SCREEN_ROLE
            && !installed.evidence_report_sha1.is_empty()
            && demand.target_glyph_count > 0
            && component_total == demand.total_slot_demand
            && demand.total_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "installed storage follow-up choice lifetime is incomplete"
    );
    Ok(TranslationLifetimeDemandReport {
        screen_role: STORAGE_FOLLOW_UP_CHOICE_SCREEN_ROLE,
        measurement_basis: "largest exact installed workset for storage dialogue record 45 after the shared yes-no glyph assignment",
        target_glyph_count: demand.target_glyph_count,
        preserved_active_source_code_count: demand.preserved_active_code_count,
        additional_target_glyph_reservation_count: 0,
        total_slot_demand: demand.total_slot_demand,
        active_slot_count: ACTIVE_HANGUL_SLOT_COUNT,
        fits_active_page: true,
        evidence_report_sha1: installed.evidence_report_sha1.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation_coverage::installed::InstalledLifetimeDemand;

    #[test]
    fn installed_storage_record_and_shared_choice_glyphs_form_one_measured_lifetime() {
        let installed = InstalledChoiceLifetime {
            screen_role: STORAGE_FOLLOW_UP_CHOICE_SCREEN_ROLE.to_owned(),
            storage_follow_up: InstalledLifetimeDemand {
                target_glyph_count: 143,
                preserved_active_code_count: 52,
                total_slot_demand: 195,
            },
            evidence_report_sha1: "exact-final-report".to_owned(),
        };

        let demand = inspect(&installed).unwrap();

        assert_eq!(demand.screen_role, STORAGE_FOLLOW_UP_CHOICE_SCREEN_ROLE);
        assert_eq!(demand.target_glyph_count, 143);
        assert_eq!(demand.preserved_active_source_code_count, 52);
        assert_eq!(demand.total_slot_demand, 195);
        assert_eq!(demand.evidence_report_sha1, "exact-final-report");
    }

    #[test]
    fn wrong_role_or_unsummed_components_cannot_measure_the_screen() {
        let mut installed = InstalledChoiceLifetime {
            screen_role: "storage_action_menu".to_owned(),
            storage_follow_up: InstalledLifetimeDemand {
                target_glyph_count: 10,
                preserved_active_code_count: 2,
                total_slot_demand: 12,
            },
            evidence_report_sha1: "exact-final-report".to_owned(),
        };
        assert!(inspect(&installed).is_err());

        installed.screen_role = STORAGE_FOLLOW_UP_CHOICE_SCREEN_ROLE.to_owned();
        installed.storage_follow_up.total_slot_demand = 11;
        assert!(inspect(&installed).is_err());
    }
}
