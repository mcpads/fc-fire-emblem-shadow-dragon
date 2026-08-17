//! 최종 패치 파이프라인의 명령행 인자와 파일 입출력 어댑터다.
//!
//! 이 모듈은 경로와 플래그를 도메인 입력 구조로 옮기고 결과를 출력할 뿐이다.
//! 원천 결속, 페이지 선택, ROM 바이트 생성과 합격 판정은 각 Rust 라이브러리가
//! 소유하고, 이 모듈은 검증된 바이트의 경로 충돌 검사·쓰기·read-back만 맡는다.

use std::path::PathBuf;

use anyhow::Result;
use clap::Args;

use crate::{full_translation_install, mapper165, translation_coverage};

mod artifact_output;
mod dispatch;

use artifact_output::write_full_translation_artifacts;

pub(super) fn execute(command: crate::Command) -> Result<()> {
    dispatch::execute(command)
}

#[derive(Debug, Args)]
pub(crate) struct AnalyzeTranslationCoverageCommand {
    source: PathBuf,
    #[arg(long, default_value = "private/dialogue/main-workspace.json")]
    main_dialogue_workspace: PathBuf,
    #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
    battle_dialogue_workspace: PathBuf,
    #[arg(long, default_value = "private/fixed-text/battle-workspace.json")]
    fixed_text_workspace: PathBuf,
    #[arg(long, default_value = "assets/translation/options.ko.json")]
    options_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/roster.ko.json")]
    roster_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/front-end-menu.ko.json")]
    front_end_menu_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/unit-names.ko.json")]
    unit_name_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/class-profiles.ko.json")]
    class_profile_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/chapter-titles.ko.json")]
    chapter_title_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/choice-labels.ko.json")]
    choice_label_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/map-menu.ko.json")]
    map_menu_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/title-logo.ko.json")]
    title_graphics_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/unit-ui-labels.ko.json")]
    unit_ui_label_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/item-action-labels.ko.json")]
    item_action_label_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/fixed-menu-labels.ko.json")]
    fixed_menu_label_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/transition-labels.ko.json")]
    transition_label_localization: PathBuf,
    #[arg(
        long,
        default_value = "evidence/private/chapter-save-lifetimes/continue-prompt-manifest.json"
    )]
    chapter_save_continue_prompt_manifest: PathBuf,
    #[arg(long, default_value = "assets/translation/location-names.ko.json")]
    location_name_localization: PathBuf,
    #[arg(long, default_value = "out/fire-emblem-fe1-korean-patch.nes")]
    current_build_output: PathBuf,
    #[arg(long, default_value = "out/kr-patch-build.json")]
    current_build_report: PathBuf,
    /// Exact final ROM emitted by the global installation planner.
    #[arg(long, default_value = "out/fire-emblem-fe1-korean-integrated.nes")]
    integrated_build_output: PathBuf,
    /// Report emitted alongside the exact final integrated ROM.
    #[arg(long, default_value = "out/full-translation-installation.json")]
    integrated_build_report: PathBuf,
    #[arg(long, default_value = "out/main-dialogue-glyph-workset.json")]
    main_dialogue_glyph_workset_report: PathBuf,
    #[arg(long, default_value = "out/battle-surface-constraints.json")]
    battle_surface_constraints_report: PathBuf,
    #[arg(long, default_value = "out/unit-ui-text.json")]
    unit_ui_text_report: PathBuf,
    #[arg(long, default_value = "out/translation-coverage.json")]
    report: PathBuf,
}

impl AnalyzeTranslationCoverageCommand {
    pub(crate) fn execute(self) -> Result<()> {
        let summary = translation_coverage::analyze_translation_coverage(
            translation_coverage::TranslationCoverageInputs {
                source_path: &self.source,
                main_dialogue_workspace_path: &self.main_dialogue_workspace,
                battle_dialogue_workspace_path: &self.battle_dialogue_workspace,
                fixed_text_workspace_path: &self.fixed_text_workspace,
                options_localization_path: &self.options_localization,
                roster_localization_path: &self.roster_localization,
                front_end_menu_localization_path: &self.front_end_menu_localization,
                unit_name_localization_path: &self.unit_name_localization,
                class_profile_localization_path: &self.class_profile_localization,
                chapter_title_localization_path: &self.chapter_title_localization,
                choice_label_localization_path: &self.choice_label_localization,
                map_menu_localization_path: &self.map_menu_localization,
                title_graphics_localization_path: &self.title_graphics_localization,
                unit_ui_label_localization_path: &self.unit_ui_label_localization,
                item_action_label_localization_path: &self.item_action_label_localization,
                fixed_menu_label_localization_path: &self.fixed_menu_label_localization,
                transition_label_localization_path: &self.transition_label_localization,
                chapter_save_continue_prompt_manifest_path: &self
                    .chapter_save_continue_prompt_manifest,
                location_name_localization_path: &self.location_name_localization,
                current_build_output_path: &self.current_build_output,
                current_build_report_path: &self.current_build_report,
                integrated_build_output_path: &self.integrated_build_output,
                integrated_build_report_path: &self.integrated_build_report,
                main_dialogue_glyph_workset_report_path: &self.main_dialogue_glyph_workset_report,
                battle_surface_constraints_report_path: &self.battle_surface_constraints_report,
                unit_ui_text_report_path: &self.unit_ui_text_report,
                report_path: &self.report,
            },
        )?;
        println!("wrote {}", self.report.display());
        println!("report SHA-1: {}", summary.report_sha1);
        println!(
            "translation coverage: {} Japanese-bearing screens, {} domains, {} unresolved source domains, {} source-bound consumer evidence domains, {} known-routes-only domains, {} complete consumer censuses, {} incomplete consumer censuses, {} domains installed for all declared consumers, {} domains runtime-bound for all declared consumers",
            summary.japanese_bearing_screen_count,
            summary.domain_count,
            summary.unresolved_source_domain_count,
            summary.source_bound_consumer_evidence_domain_count,
            summary.known_routes_only_domain_count,
            summary.complete_consumer_census_domain_count,
            summary.incomplete_consumer_census_domain_count,
            summary.all_declared_consumers_installed_domain_count,
            summary.all_declared_consumers_runtime_bound_domain_count
        );
        Ok(())
    }
}

#[derive(Debug, Args)]
pub(crate) struct PlanFullTranslationInstallationCommand {
    source: PathBuf,
    #[arg(long, alias = "transport-probe")]
    output: Option<PathBuf>,
    #[arg(long, default_value = "private/dialogue/main-workspace.json")]
    main_dialogue_workspace: PathBuf,
    #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
    battle_dialogue_workspace: PathBuf,
    #[arg(long, default_value = "private/fixed-text/battle-workspace.json")]
    fixed_text_workspace: PathBuf,
    #[arg(long, default_value = "assets/translation/options.ko.json")]
    options_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/roster.ko.json")]
    roster_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/front-end-menu.ko.json")]
    front_end_menu_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/class-profiles.ko.json")]
    class_profile_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/title-logo.ko.json")]
    title_graphics_localization: PathBuf,
    #[arg(long, default_value = "out/title-logo.asset")]
    title_logo_asset: PathBuf,
    #[arg(long, default_value = "assets/translation/unit-names.ko.json")]
    unit_name_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/chapter-titles.ko.json")]
    chapter_title_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/choice-labels.ko.json")]
    choice_label_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/map-menu.ko.json")]
    map_menu_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/unit-ui-labels.ko.json")]
    unit_ui_label_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/item-action-labels.ko.json")]
    item_action_label_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/fixed-menu-labels.ko.json")]
    fixed_menu_label_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/transition-labels.ko.json")]
    transition_label_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/location-names.ko.json")]
    location_name_localization: PathBuf,
    #[arg(long, default_value = "out/fire-emblem-fe1-korean-patch.nes")]
    current_candidate: PathBuf,
    #[arg(long, default_value = "out/kr-patch-build.json")]
    current_build_report: PathBuf,
    /// Bind private cold-route observations to the exact integrated image.
    #[arg(long)]
    final_runtime_evidence: Option<PathBuf>,
    #[arg(long, default_value = "out/full-translation-installation.json")]
    report: PathBuf,
}

impl PlanFullTranslationInstallationCommand {
    pub(crate) fn execute(self) -> Result<()> {
        let artifacts = full_translation_install::plan_full_translation_installation(
            full_translation_install::FullTranslationInstallInputs {
                source_path: &self.source,
                main_dialogue_workspace_path: &self.main_dialogue_workspace,
                battle_dialogue_workspace_path: &self.battle_dialogue_workspace,
                fixed_text_workspace_path: &self.fixed_text_workspace,
                options_localization_path: &self.options_localization,
                roster_localization_path: &self.roster_localization,
                front_end_menu_localization_path: &self.front_end_menu_localization,
                class_profile_localization_path: &self.class_profile_localization,
                title_graphics_localization_path: &self.title_graphics_localization,
                title_logo_asset_path: &self.title_logo_asset,
                unit_name_localization_path: &self.unit_name_localization,
                chapter_title_localization_path: &self.chapter_title_localization,
                choice_label_localization_path: &self.choice_label_localization,
                map_menu_localization_path: &self.map_menu_localization,
                unit_ui_label_localization_path: &self.unit_ui_label_localization,
                item_action_label_localization_path: &self.item_action_label_localization,
                fixed_menu_label_localization_path: &self.fixed_menu_label_localization,
                transition_label_localization_path: &self.transition_label_localization,
                location_name_localization_path: &self.location_name_localization,
                current_candidate_path: &self.current_candidate,
                current_build_report_path: &self.current_build_report,
                final_runtime_evidence_path: self.final_runtime_evidence.as_deref(),
                output_will_be_emitted: self.output.is_some(),
            },
        )?;
        write_full_translation_artifacts(
            self.output.as_deref(),
            &self.report,
            &self.source,
            &self.current_candidate,
            &artifacts.integrated_image,
            &artifacts.report_bytes,
        )?;
        let summary = artifacts.summary;
        println!("wrote {}", self.report.display());
        println!("report SHA-1: {}", summary.report_sha1);
        println!(
            "full translation installation: {} declared installation domains, {} dialogue records, {} page worksets, {} glyphs with a {}-page static upper bound and maximum {}-slot page demand, {} pointer writes, {} planned bytes",
            summary.declared_installation_domain_count,
            summary.dialogue_record_count,
            summary.dialogue_page_workset_count,
            summary.dialogue_glyph_count,
            summary.dialogue_static_page_upper_bound_count,
            summary.dialogue_maximum_page_slot_demand,
            summary.dialogue_pointer_write_count,
            summary.dialogue_planned_storage_byte_count,
        );
        println!("integrated image SHA-1: {}", summary.integrated_image_sha1);
        Ok(())
    }
}

#[derive(Debug, Args)]
pub(crate) struct BuildCumulativePatchCommand {
    source: PathBuf,
    #[arg(long, default_value = "assets/translation/options.ko.json")]
    options_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/roster.ko.json")]
    roster_localization: PathBuf,
    #[arg(
        long,
        default_value = "evidence/private/hangul-page-context/options-lifetime-manifest.json"
    )]
    options_screen_evidence: PathBuf,
    #[arg(long, default_value = "private/dialogue/main-workspace.json")]
    main_dialogue_workspace: PathBuf,
    #[arg(long, default_value = "assets/translation/chapter-titles.ko.json")]
    chapter_title_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/front-end-menu.ko.json")]
    front_end_menu_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/unit-names.ko.json")]
    unit_name_localization: PathBuf,
    #[arg(long, default_value = "assets/translation/class-profiles.ko.json")]
    class_profile_localization: PathBuf,
    #[arg(long, default_value = "private/fixed-text/battle-workspace.json")]
    fixed_text_workspace: PathBuf,
    #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
    battle_dialogue_workspace: PathBuf,
    #[arg(
        long,
        default_value = "evidence/private/temporal-surfaces/manifest.json"
    )]
    battle_temporal_manifest: PathBuf,
    #[arg(long, default_value = "assets/translation/choice-labels.ko.json")]
    choice_label_localization: PathBuf,
    #[arg(
        long,
        default_value = "evidence/private/dialogue-lifetime/chapter-1-intro-screen.json"
    )]
    chapter_one_intro_evidence: PathBuf,
    #[arg(
        long,
        default_value = "evidence/private/cumulative-chapter2/chapter-2-intro-screen.json"
    )]
    chapter_two_intro_evidence: PathBuf,
    #[arg(long, default_value = "evidence/private/front-end-menu/manifest.json")]
    front_end_menu_evidence: PathBuf,
    #[arg(
        long,
        default_value = "evidence/private/unit-status-contract/unit-name-manifest.json"
    )]
    unit_name_evidence: PathBuf,
    #[arg(long, default_value = "evidence/private/class-profile-manifest.json")]
    class_profile_evidence: PathBuf,
    #[arg(
        long,
        default_value = "evidence/private/class-profile-installed/manifest.json"
    )]
    class_profile_runtime_evidence: PathBuf,
    #[arg(long, default_value = "private/runtime/shop-dialogue-screen.json")]
    shop_dialogue_evidence: PathBuf,
    #[arg(
        long,
        default_value = "evidence/private/shop-dialogue-installed/manifest.json"
    )]
    shop_dialogue_runtime_evidence: PathBuf,
    #[arg(
        long,
        default_value = "evidence/private/shop-shared-text-installed/manifest.json"
    )]
    weapon_shop_shared_text_runtime_evidence: PathBuf,
    #[arg(
        long,
        default_value = "evidence/private/chapter7-maximum-lifetime/manifest.json"
    )]
    maximum_dialogue_evidence: PathBuf,
    #[arg(
        long,
        default_value = "evidence/private/chapter7-maximum-installed/page-boundaries.json"
    )]
    maximum_dialogue_page_boundaries: PathBuf,
    #[arg(long)]
    maximum_dialogue_runtime_evidence: Option<PathBuf>,
    #[arg(long, default_value = "assets/translation/title-logo.ko.json")]
    title_graphics_localization: PathBuf,
    #[arg(long, default_value = "out/title-logo.asset")]
    title_logo_asset: PathBuf,
    #[arg(
        long,
        default_value = "evidence/private/title-logo-runtime-completion/manifest.json"
    )]
    title_logo_runtime_evidence: PathBuf,
    /// Emit a development build while leaving exact-output runtime bindings unresolved.
    #[arg(long, default_value_t = false)]
    defer_runtime_evidence: bool,
    #[arg(long, default_value = "out/cumulative-stages")]
    stage_directory: PathBuf,
    #[arg(long, default_value = "out/fire-emblem-fe1-korean-patch.nes")]
    output: PathBuf,
    #[arg(long, default_value = "out/kr-patch-build.json")]
    report: PathBuf,
}

impl BuildCumulativePatchCommand {
    pub(crate) fn execute(self) -> Result<()> {
        let summary = mapper165::cumulative_patch::build_cumulative_patch(
            mapper165::cumulative_patch::CumulativePatchInputs {
                source_path: &self.source,
                options_localization_path: &self.options_localization,
                roster_localization_path: &self.roster_localization,
                options_screen_evidence_path: &self.options_screen_evidence,
                main_dialogue_workspace_path: &self.main_dialogue_workspace,
                chapter_title_localization_path: &self.chapter_title_localization,
                front_end_menu_localization_path: &self.front_end_menu_localization,
                unit_name_localization_path: &self.unit_name_localization,
                class_profile_localization_path: &self.class_profile_localization,
                fixed_text_workspace_path: &self.fixed_text_workspace,
                battle_dialogue_workspace_path: &self.battle_dialogue_workspace,
                battle_temporal_manifest_path: &self.battle_temporal_manifest,
                choice_label_localization_path: &self.choice_label_localization,
                chapter_one_intro_evidence_path: &self.chapter_one_intro_evidence,
                chapter_two_intro_evidence_path: &self.chapter_two_intro_evidence,
                front_end_menu_evidence_path: &self.front_end_menu_evidence,
                unit_name_evidence_path: &self.unit_name_evidence,
                class_profile_evidence_path: &self.class_profile_evidence,
                class_profile_runtime_evidence_path: (!self.defer_runtime_evidence)
                    .then_some(self.class_profile_runtime_evidence.as_path()),
                shop_dialogue_evidence_path: &self.shop_dialogue_evidence,
                shop_dialogue_runtime_evidence_path: (!self.defer_runtime_evidence)
                    .then_some(self.shop_dialogue_runtime_evidence.as_path()),
                weapon_shop_shared_text_runtime_evidence_path: (!self.defer_runtime_evidence)
                    .then_some(self.weapon_shop_shared_text_runtime_evidence.as_path()),
                maximum_dialogue_evidence_path: &self.maximum_dialogue_evidence,
                maximum_dialogue_page_boundary_path: &self.maximum_dialogue_page_boundaries,
                maximum_dialogue_runtime_evidence_path: (!self.defer_runtime_evidence)
                    .then_some(self.maximum_dialogue_runtime_evidence.as_deref())
                    .flatten(),
                title_graphics_localization_path: &self.title_graphics_localization,
                title_logo_asset_path: &self.title_logo_asset,
                title_logo_runtime_evidence_path: (!self.defer_runtime_evidence)
                    .then_some(self.title_logo_runtime_evidence.as_path()),
                stage_directory: &self.stage_directory,
                output_path: &self.output,
                report_path: &self.report,
            },
        )?;
        println!("wrote {}", self.output.display());
        println!("output SHA-1: {}", summary.output_sha1);
        println!("wrote {}", self.report.display());
        println!("report SHA-1: {}", summary.report_sha1);
        println!(
            "cumulative patch: {} stages, {} installed chapter titles, {} installed dialogue records, {} installed translated lines, {} installed glyph slots, {} tracked writes",
            summary.stage_count,
            summary.installed_chapter_title_count,
            summary.installed_dialogue_record_count,
            summary.installed_dialogue_line_count,
            summary.installed_glyph_slot_count,
            summary.tracked_write_count
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::{Cli, Command};

    #[test]
    fn cumulative_build_cli_keeps_its_public_defaults_and_deferred_evidence_flag() {
        let cli = Cli::try_parse_from([
            "fc-fire-emblem-patch",
            "build-kr-patch",
            "source.nes",
            "--defer-runtime-evidence",
        ])
        .unwrap();
        let Command::BuildKrPatch(command) = cli.command else {
            panic!("parsed the wrong command");
        };

        assert_eq!(command.source, PathBuf::from("source.nes"));
        assert!(command.defer_runtime_evidence);
        assert_eq!(
            command.output,
            PathBuf::from("out/fire-emblem-fe1-korean-patch.nes")
        );
        assert_eq!(command.report, PathBuf::from("out/kr-patch-build.json"));
    }

    #[test]
    fn final_install_cli_preserves_the_output_alias_and_artifact_inputs() {
        let cli = Cli::try_parse_from([
            "fc-fire-emblem-patch",
            "plan-full-translation-installation",
            "source.nes",
            "--transport-probe",
            "final.nes",
            "--current-candidate",
            "cumulative.nes",
            "--current-build-report",
            "cumulative.json",
        ])
        .unwrap();
        let Command::PlanFullTranslationInstallation(command) = cli.command else {
            panic!("parsed the wrong command");
        };

        assert_eq!(command.output, Some(PathBuf::from("final.nes")));
        assert_eq!(command.current_candidate, PathBuf::from("cumulative.nes"));
        assert_eq!(
            command.current_build_report,
            PathBuf::from("cumulative.json")
        );
    }

    #[test]
    fn coverage_cli_keeps_post_build_inputs_separate_from_its_report() {
        let cli = Cli::try_parse_from([
            "fc-fire-emblem-patch",
            "analyze-translation-coverage",
            "source.nes",
            "--current-build-output",
            "cumulative.nes",
            "--integrated-build-output",
            "final.nes",
            "--report",
            "coverage.json",
        ])
        .unwrap();
        let Command::AnalyzeTranslationCoverage(command) = cli.command else {
            panic!("parsed the wrong command");
        };

        assert_eq!(
            command.current_build_output,
            PathBuf::from("cumulative.nes")
        );
        assert_eq!(command.integrated_build_output, PathBuf::from("final.nes"));
        assert_eq!(command.report, PathBuf::from("coverage.json"));
    }
}
