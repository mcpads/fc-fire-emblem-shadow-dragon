mod battle_text_workset;
mod chapter_transition;
mod chapter_victory;
mod choice_labels;
mod chr_inventory;
mod class_profile;
mod dialogue_assets;
mod dialogue_inventory;
mod epilogue_variant_evidence;
mod font;
mod font_slots;
mod front_end_menu;
mod full_translation_install;
mod hangul_page_plan;
mod item_flow;
mod japanese_encoding;
mod localization;
mod map_menu;
mod mapper165;
mod mmc4_latch;
mod mmc5_chr;
mod mmc5_expanded_chr;
mod mmc5_exram_probe;
mod mmc5_nametable_shadow;
mod mmc5_prg;
mod mmc5_queue_runtime;
mod mmc5_queue_shadow;
mod options;
mod release_image;
mod rom;
mod roster_localization;
mod rp2a03;
mod screen_contracts;
mod semantic_translation;
mod shop_flow;
mod source_font_page;
mod source_literals;
mod static_analysis;
mod suspend_message;
mod temporal_surface;
#[cfg(test)]
mod test_support;
mod text_inventory;
mod title_graphics;
mod tracked;
mod translation_coverage;
mod typed_source;
mod unit_names;
mod unit_ui_text;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(about = "Build and verify the FE1 Japanese-to-Korean patch")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

// clap constructs one command payload at process startup; boxing individual path fields would
// complicate argument plumbing without reducing a persistent runtime allocation.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Subcommand)]
enum Command {
    /// Verify that a ROM is the exact supported Japanese source revision.
    VerifySource { source: PathBuf },
    /// Repack a cumulative image with an NES 2.0 header and mapper-maximum CHR alignment.
    BuildReleaseImage {
        cumulative: PathBuf,
        #[arg(long, default_value = "out/fire-emblem-fe1-korean-release.nes")]
        output: PathBuf,
        #[arg(long, default_value = "out/release-image.json")]
        report: PathBuf,
    },
    /// Analyze the supported source font page without declaring free slots.
    AnalyzeFontSupply {
        source: PathBuf,
        #[arg(long, default_value = "out/font-supply.json")]
        report: PathBuf,
        #[arg(long, default_value = "out/font-page-00.png")]
        sheet: PathBuf,
        #[arg(long, default_value_t = 4)]
        scale: u32,
    },
    /// Inventory confirmed Japanese-source name pointer tables without translating English codes.
    AnalyzeTextTables {
        source: PathBuf,
        #[arg(long, default_value = "out/text-tables.json")]
        report: PathBuf,
    },
    /// Create a private workspace for battle names, terrain, and message templates.
    ExtractFixedTextWorkspace {
        source: PathBuf,
        #[arg(long, default_value = "private/fixed-text/battle-workspace.json")]
        output: PathBuf,
    },
    /// Create the small public workspace for the front-end menu label family.
    ExtractFrontEndMenuWorkspace {
        source: PathBuf,
        #[arg(long, default_value = "assets/translation/front-end-menu.ko.json")]
        output: PathBuf,
    },
    /// Create the small public workspace for playable-unit names.
    ExtractUnitNameWorkspace {
        source: PathBuf,
        #[arg(long, default_value = "assets/translation/unit-names.ko.json")]
        output: PathBuf,
    },
    /// Create the small public workspace for all location names.
    ExtractLocationNameWorkspace {
        source: PathBuf,
        #[arg(long, default_value = "assets/translation/location-names.ko.json")]
        output: PathBuf,
    },
    /// Create the source-bound workspace for all automatic class profiles.
    ExtractClassProfileWorkspace {
        source: PathBuf,
        #[arg(long, default_value = "assets/translation/class-profiles.ko.json")]
        output: PathBuf,
    },
    /// Measure translated battle names, classes, items, and messages without emitting text.
    AnalyzeBattleTextWorkset {
        source: PathBuf,
        #[arg(long, default_value = "private/fixed-text/battle-workspace.json")]
        fixed_workspace: PathBuf,
        #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
        dialogue_workspace: PathBuf,
        #[arg(long, default_value = "out/battle-text-workset.json")]
        report: PathBuf,
    },
    /// Test whether all battle glyphs can keep stable byte codes across cache pages.
    AnalyzeBattleCodebookPlan {
        source: PathBuf,
        #[arg(long, default_value = "private/fixed-text/battle-workspace.json")]
        fixed_workspace: PathBuf,
        #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
        dialogue_workspace: PathBuf,
        #[arg(long, default_value = "out/battle-codebook-plan.json")]
        report: PathBuf,
    },
    /// Bind observed battle-animation tile protection to runtime recipe selections.
    AnalyzeBattleSurfaceConstraints {
        source: PathBuf,
        #[arg(long, default_value = "private/fixed-text/battle-workspace.json")]
        fixed_workspace: PathBuf,
        #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
        dialogue_workspace: PathBuf,
        #[arg(
            long,
            default_value = "evidence/private/temporal-surfaces/manifest.json"
        )]
        temporal_manifest: PathBuf,
        #[arg(long, default_value = "out/battle-surface-constraints.json")]
        report: PathBuf,
    },
    /// Expand mapper 165 PRG and embed the translated battle glyph atlas.
    BuildBattleTextCacheBase {
        source: PathBuf,
        #[arg(long, default_value = "private/fixed-text/battle-workspace.json")]
        fixed_workspace: PathBuf,
        #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
        dialogue_workspace: PathBuf,
        #[arg(long, default_value = "out/battle-text-cache-base.nes")]
        output: PathBuf,
        #[arg(long, default_value = "out/battle-text-cache-base.json")]
        report: PathBuf,
    },
    /// Reinsert every translated battle string and embed its observed physical codebook.
    BuildBattleTextRuntimeBase {
        source: PathBuf,
        #[arg(long, default_value = "private/fixed-text/battle-workspace.json")]
        fixed_workspace: PathBuf,
        #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
        dialogue_workspace: PathBuf,
        #[arg(
            long,
            default_value = "evidence/private/temporal-surfaces/manifest.json"
        )]
        temporal_manifest: PathBuf,
        #[arg(long, default_value = "out/battle-text-runtime-base.nes")]
        output: PathBuf,
        #[arg(long, default_value = "out/battle-text-runtime-base.json")]
        report: PathBuf,
    },
    /// Compose observed battle recipes into CHR RAM behind an exact runtime-tuple gate.
    BuildBattleCompositionLoaderProbe {
        source: PathBuf,
        #[arg(
            long,
            default_value = "evidence/private/temporal-surfaces/manifest.json"
        )]
        temporal_manifest: PathBuf,
        #[arg(long, default_value = "out/battle-text-runtime-base.nes")]
        base: PathBuf,
        #[arg(long, default_value = "out/battle-text-runtime-base.json")]
        base_report: PathBuf,
        #[arg(long, default_value = "out/battle-composition-loader-probe.nes")]
        output: PathBuf,
        #[arg(long, default_value = "out/battle-composition-loader-probe.json")]
        report: PathBuf,
    },
    /// Compare a composition-return CHR-RAM snapshot with an independent recipe rebuild.
    VerifyBattleCompositionRuntime {
        #[arg(long, default_value = "out/battle-composition-loader-probe.nes")]
        rom: PathBuf,
        event: PathBuf,
        #[arg(
            long,
            default_value = "out/battle-composition-runtime-verification.json"
        )]
        report: PathBuf,
    },
    /// Build one proven battle combination with fixed text and dialogue sharing a codebook.
    BuildBattleCombinationProbe {
        source: PathBuf,
        #[arg(long, default_value = "private/fixed-text/battle-workspace.json")]
        fixed_workspace: PathBuf,
        #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
        dialogue_workspace: PathBuf,
        #[arg(long, default_value = "out/battle-combination-probe.nes")]
        output: PathBuf,
        #[arg(long, default_value = "out/battle-combination-probe.json")]
        report: PathBuf,
    },
    /// Upload one proven battle codebook into mapper 165 CHR RAM at the battle transition.
    BuildBattleCacheUploadProbe {
        source: PathBuf,
        #[arg(long, default_value = "private/fixed-text/battle-workspace.json")]
        fixed_workspace: PathBuf,
        #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
        dialogue_workspace: PathBuf,
        #[arg(long, default_value = "out/battle-cache-upload-probe.nes")]
        output: PathBuf,
        #[arg(long, default_value = "out/battle-cache-upload-probe.json")]
        report: PathBuf,
    },
    /// Inventory dialogue entry tables without emitting source dialogue bytes.
    AnalyzeDialogueStructure {
        source: PathBuf,
        #[arg(long, default_value = "out/dialogue-structure.json")]
        report: PathBuf,
    },
    /// Inventory screen-level text, graphics, temporal UI, input, and font-lifetime contracts.
    AnalyzeScreenContracts {
        source: PathBuf,
        #[arg(long, default_value = "out/screen-contracts.json")]
        report: PathBuf,
    },
    /// Connect every Japanese-bearing screen to translation input and current installation status.
    AnalyzeTranslationCoverage {
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
        #[arg(long, default_value = "out/main-dialogue-glyph-workset.json")]
        main_dialogue_glyph_workset_report: PathBuf,
        #[arg(long, default_value = "out/battle-surface-constraints.json")]
        battle_surface_constraints_report: PathBuf,
        #[arg(long, default_value = "out/unit-ui-text.json")]
        unit_ui_text_report: PathBuf,
        #[arg(long, default_value = "out/translation-coverage.json")]
        report: PathBuf,
    },
    /// Plan every unfinished translation domain together before emitting one integrated ROM.
    PlanFullTranslationInstallation {
        source: PathBuf,
        #[arg(long, default_value = "private/dialogue/main-workspace.json")]
        main_dialogue_workspace: PathBuf,
        #[arg(long, default_value = "private/fixed-text/battle-workspace.json")]
        fixed_text_workspace: PathBuf,
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
        #[arg(long, default_value = "assets/translation/transition-labels.ko.json")]
        transition_label_localization: PathBuf,
        #[arg(long, default_value = "assets/translation/location-names.ko.json")]
        location_name_localization: PathBuf,
        #[arg(long, default_value = "out/fire-emblem-fe1-korean-patch.nes")]
        current_candidate: PathBuf,
        #[arg(long, default_value = "out/kr-patch-build.json")]
        current_build_report: PathBuf,
        #[arg(long, default_value = "out/full-translation-installation.json")]
        report: PathBuf,
    },
    /// Bind chapter-clear, save, title, and chapter-intro screen lifetimes and producers.
    AnalyzeChapterTransitions {
        source: PathBuf,
        #[arg(long, default_value = "out/chapter-transitions.json")]
        report: PathBuf,
    },
    /// Create a source-bound workspace for all twenty-five Japanese chapter titles.
    ExtractChapterTitleWorkspace {
        source: PathBuf,
        #[arg(long, default_value = "assets/translation/chapter-titles.ko.json")]
        output: PathBuf,
    },
    /// Aggregate private frozen-frame dumps into battle and ending temporal unions.
    AnalyzeTemporalSurfaces {
        source: PathBuf,
        manifest: PathBuf,
        #[arg(long, default_value = "out/temporal-surfaces.json")]
        report: PathBuf,
    },
    /// Validate character-epilogue variants without emitting dialogue or evidence paths.
    AnalyzeEpilogueVariants {
        source: PathBuf,
        captures: PathBuf,
        #[arg(long, default_value = "out/fire-emblem-fe1-mapper165-parity-probe.nes")]
        capture_rom: PathBuf,
        #[arg(long, default_value = "out/mapper165-parity-probe.json")]
        mapper_report: PathBuf,
        #[arg(long, default_value = "out/ending-epilogue-variants.json")]
        report: PathBuf,
    },
    /// Bind chapter-eleven victory tiles and the castle-command-to-transition route.
    AnalyzeChapterVictory {
        source: PathBuf,
        #[arg(long, default_value = "out/chapter-victory-route.json")]
        report: PathBuf,
    },
    /// Bind weapon-shop entry, item selection, preflight, confirmation, and mutation boundaries.
    AnalyzeShopFlow {
        source: PathBuf,
        #[arg(long, default_value = "out/shop-flow.json")]
        report: PathBuf,
    },
    /// Bind unit-command inventory entry, item rows, conditional actions, and mutations.
    AnalyzeItemFlow {
        source: PathBuf,
        #[arg(long, default_value = "out/item-flow.json")]
        report: PathBuf,
    },
    /// Bind unit-summary and unit-status composers, sources, labels, and shared page lifetime.
    AnalyzeUnitUiText {
        source: PathBuf,
        #[arg(long, default_value = "private/fixed-text/battle-workspace.json")]
        fixed_text_workspace: PathBuf,
        #[arg(long, default_value = "assets/translation/unit-names.ko.json")]
        unit_name_localization: PathBuf,
        #[arg(long, default_value = "assets/translation/unit-ui-labels.ko.json")]
        unit_ui_label_localization: PathBuf,
        #[arg(long, default_value = "out/unit-ui-text.json")]
        report: PathBuf,
    },
    /// Extract exact main-dialogue source storage for a private roundtrip check.
    ExtractMainDialogueSource {
        source: PathBuf,
        #[arg(long, default_value = "private/dialogue/main-source.json")]
        output: PathBuf,
    },
    /// Create a private Japanese-to-Korean main-dialogue translation workspace.
    ExtractMainDialogueWorkspace {
        source: PathBuf,
        #[arg(long, default_value = "private/dialogue/main-workspace.json")]
        output: PathBuf,
    },
    /// Create a private Japanese-to-Korean battle-dialogue translation workspace.
    ExtractBattleDialogueWorkspace {
        source: PathBuf,
        #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
        output: PathBuf,
    },
    /// Validate private battle-dialogue translations without writing a ROM.
    ValidateBattleDialogueWorkspace {
        source: PathBuf,
        #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
        workspace: PathBuf,
    },
    /// Import a complete private first-pass battle-dialogue TSV.
    ImportBattleDialogueDraft {
        #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
        workspace: PathBuf,
        #[arg(long, default_value = "private/dialogue/battle-draft.tsv")]
        draft: PathBuf,
    },
    /// Plan battle-dialogue relocation while preserving the unreferenced physical record.
    PlanBattleDialogueReinsertion {
        source: PathBuf,
        #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
        workspace: PathBuf,
        #[arg(long, default_value = "private/dialogue/battle-layout.json")]
        report: PathBuf,
    },
    /// Build every translated battle-dialogue record as a mapper 165 development probe.
    BuildBattleDialogueProbe {
        source: PathBuf,
        #[arg(long, default_value = "private/dialogue/battle-workspace.json")]
        workspace: PathBuf,
        #[arg(long, default_value = "out/battle-dialogue-probe.nes")]
        output: PathBuf,
        #[arg(long, default_value = "out/battle-dialogue-probe.json")]
        report: PathBuf,
    },
    /// Validate private translations without encoding or writing a ROM.
    ValidateMainDialogueWorkspace {
        source: PathBuf,
        #[arg(long, default_value = "private/dialogue/main-workspace.json")]
        workspace: PathBuf,
    },
    /// Count reviewed Korean glyphs without emitting dialogue or glyph characters.
    AnalyzeMainDialogueGlyphWorkset {
        source: PathBuf,
        #[arg(long, default_value = "private/dialogue/main-workspace.json")]
        workspace: PathBuf,
        #[arg(
            long,
            default_value = "evidence/private/chapter7-maximum-lifetime/manifest.json"
        )]
        maximum_lifetime_evidence: PathBuf,
        #[arg(long, default_value = "out/main-dialogue-glyph-workset.json")]
        report: PathBuf,
    },
    /// Plan variable-length main-dialogue storage without encoding or writing a ROM.
    PlanMainDialogueReinsertion {
        source: PathBuf,
        #[arg(long, default_value = "private/dialogue/main-workspace.json")]
        workspace: PathBuf,
        #[arg(long, default_value = "out/main-dialogue-layout.json")]
        report: PathBuf,
    },
    /// Verify that a private main-dialogue source asset rebuilds the source exactly.
    VerifyMainDialogueSourceRoundtrip {
        source: PathBuf,
        #[arg(long, default_value = "private/dialogue/main-source.json")]
        asset: PathBuf,
        #[arg(long, default_value = "out/fire-emblem-fe1-dialogue-roundtrip.nes")]
        output: PathBuf,
    },
    /// Convert an ImageGen title concept into source-owned NES logo tiles and phase previews.
    BuildTitleLogoAsset {
        source: PathBuf,
        #[arg(long, default_value = "private/title/logo-candidate-plan.json")]
        manifest: PathBuf,
        #[arg(long, default_value = "out/title-logo.asset")]
        asset: PathBuf,
        #[arg(long, default_value = "out/title-logo-phases.png")]
        preview: PathBuf,
        #[arg(long, default_value = "out/title-logo-asset.json")]
        report: PathBuf,
    },
    /// Build the Japanese-options Hangul visibility proof.
    BuildOptionsPoc {
        source: PathBuf,
        #[arg(long, default_value = "assets/translation/options.ko.json")]
        localization: PathBuf,
        #[arg(long, default_value = "out/fire-emblem-fe1-options-poc.nes")]
        output: PathBuf,
        #[arg(long, default_value = "out/fire-emblem-fe1-options-poc.png")]
        preview: PathBuf,
        #[arg(long, default_value_t = 8)]
        preview_scale: u32,
    },
    /// Convert FE1 to the MMC2+MMC3 hybrid mapper 165 without translation assets.
    BuildMapper165ParityProbe {
        source: PathBuf,
        #[arg(long, default_value = "out/fire-emblem-fe1-mapper165-parity-probe.nes")]
        output: PathBuf,
        #[arg(long, default_value = "out/mapper165-parity-probe.json")]
        report: PathBuf,
    },
    /// Plan two expanded-CHR Hangul pages whose union exceeds one active page.
    PlanHangulPageProof {
        source: PathBuf,
        #[arg(long, default_value = "assets/translation/options.ko.json")]
        localization: PathBuf,
        #[arg(long, default_value = "out/hangul-page-proof.chr")]
        page_pack: PathBuf,
        #[arg(long, default_value = "out/hangul-page-proof.json")]
        report: PathBuf,
    },
    /// Build mapper 165 with options and mixed-text roster Hangul pages.
    BuildMapper165HangulPageProbe {
        source: PathBuf,
        #[arg(long, default_value = "assets/translation/options.ko.json")]
        localization: PathBuf,
        #[arg(long, default_value = "assets/translation/roster.ko.json")]
        roster_localization: PathBuf,
        #[arg(
            long,
            default_value = "evidence/private/hangul-page-context/options-lifetime-manifest.json"
        )]
        options_screen_evidence: PathBuf,
        #[arg(
            long,
            default_value = "out/fire-emblem-fe1-mapper165-hangul-page-probe.nes"
        )]
        output: PathBuf,
        #[arg(long, default_value = "out/mapper165-hangul-page-probe.json")]
        report: PathBuf,
    },
    /// Build the cumulative mapper 165 Korean patch lineage from the supported source.
    BuildKrPatch {
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
        #[arg(long, default_value = "out/cumulative-stages")]
        stage_directory: PathBuf,
        #[arg(long, default_value = "out/fire-emblem-fe1-korean-patch.nes")]
        output: PathBuf,
        #[arg(long, default_value = "out/kr-patch-build.json")]
        report: PathBuf,
    },
    /// Build one reviewed main-dialogue record as an end-to-end mapper 165 development probe.
    BuildMainDialogueSliceProbe {
        source: PathBuf,
        #[arg(long, default_value = "private/dialogue/main-workspace.json")]
        workspace: PathBuf,
        #[arg(
            long,
            default_value = "evidence/private/dialogue-lifetime/chapter-1-intro-screen.json"
        )]
        screen_evidence: PathBuf,
        #[arg(long, default_value = "chapter-intro-dialogue:000")]
        record_id: String,
        #[arg(
            long,
            default_value = "out/fire-emblem-fe1-main-dialogue-slice-probe.nes"
        )]
        output: PathBuf,
        #[arg(long, default_value = "out/main-dialogue-slice-probe.json")]
        report: PathBuf,
    },
    /// Compare MMC4 and mapper 165 FD-trigger tile planes for observed CHR pairs.
    AnalyzeMapper165TriggerPlanes {
        source: PathBuf,
        #[arg(long, default_value = "out/mapper165-trigger-planes.json")]
        report: PathBuf,
    },
    /// Classify direct CHR writers by their accumulator-value pairing contract.
    AnalyzeMapper165DirectChrPairs {
        source: PathBuf,
        #[arg(long, default_value = "out/mapper165-direct-chr-pairs.json")]
        report: PathBuf,
    },
    /// Build the static MMC5 PRG and SRAM conversion probe without translation assets.
    BuildMmc5PrgProbe {
        source: PathBuf,
        #[arg(long, default_value = "out/fire-emblem-fe1-mmc5-prg-probe.nes")]
        output: PathBuf,
        #[arg(long, default_value = "out/mmc5-prg-probe.json")]
        report: PathBuf,
    },
    /// Project runtime-proven MMC4 CHR writers onto MMC5 4 KiB banks.
    BuildMmc5ChrWriterProbe {
        source: PathBuf,
        #[arg(long, default_value = "out/fire-emblem-fe1-mmc5-chr-writer-probe.nes")]
        output: PathBuf,
        #[arg(long, default_value = "out/mmc5-chr-writer-probe.json")]
        report: PathBuf,
    },
    /// Build a 256 KiB CHR copy and serve the Korean options proof from its upper half.
    BuildMmc5ExpandedChrOptionsProbe {
        source: PathBuf,
        #[arg(long, default_value = "assets/translation/options.ko.json")]
        localization: PathBuf,
        #[arg(
            long,
            default_value = "out/fire-emblem-fe1-mmc5-expanded-chr-options-probe.nes"
        )]
        output: PathBuf,
        #[arg(long, default_value = "out/mmc5-expanded-chr-options-probe.json")]
        report: PathBuf,
    },
    /// Embed one proven dialogue-screen latch projection and load it through MMC5 ExRAM.
    BuildMmc5DialogueExramProbe {
        source: PathBuf,
        attributes: PathBuf,
        #[arg(
            long,
            default_value = "out/fire-emblem-fe1-mmc5-dialogue-exram-probe.nes"
        )]
        output: PathBuf,
        #[arg(long, default_value = "out/mmc5-dialogue-exram-probe.json")]
        report: PathBuf,
    },
    /// Mirror every direct PPU address/data store into isolated MMC5 PRG RAM.
    BuildMmc5NametableShadowProbe {
        source: PathBuf,
        #[arg(
            long,
            default_value = "out/fire-emblem-fe1-mmc5-nametable-shadow-probe.nes"
        )]
        output: PathBuf,
        #[arg(long, default_value = "out/mmc5-nametable-shadow-probe.json")]
        report: PathBuf,
    },
    /// Replay confirmed PPU queues at their publish boundaries into MMC5 PRG RAM.
    BuildMmc5QueueShadowProbe {
        source: PathBuf,
        #[arg(
            long,
            default_value = "out/fire-emblem-fe1-mmc5-queue-shadow-probe.nes"
        )]
        output: PathBuf,
        #[arg(long, default_value = "out/mmc5-queue-shadow-probe.json")]
        report: PathBuf,
    },
    /// Project one zero-scroll MMC4 nametable into MMC5 extended attributes.
    ProjectMmc4LatchNametable {
        input: PathBuf,
        #[arg(long, default_value_t = 0)]
        nametable_index: usize,
        #[arg(long)]
        fd_bank: u8,
        #[arg(long)]
        fe_bank: u8,
        #[arg(long, value_enum)]
        initial_latch: mmc4_latch::Mmc4Latch,
        #[arg(long, default_value = "out/mmc5-exram-attributes.bin")]
        output: PathBuf,
        #[arg(long, default_value = "out/mmc4-latch-nametable.json")]
        report: PathBuf,
    },
    /// Replay captured PPU transfers into mirrored nametables and project one viewport.
    ReplayMmc4LatchPpuTransfers {
        input: PathBuf,
        #[arg(long, default_value = "out/mmc5-exram-transfer-replay.bin")]
        output: PathBuf,
        #[arg(long, default_value = "out/mmc4-latch-transfer-replay.json")]
        report: PathBuf,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::VerifySource { source } => {
            let source_rom = rom::Rom::from_path(&source)?;
            source_rom.verify_supported_japanese()?;
            println!(
                "verified Japanese source: SHA-1 {} (mapper {}, PRG {} bytes, CHR {} bytes)",
                rom::EXPECTED_SOURCE_SHA1,
                source_rom.mapper(),
                source_rom.prg().len(),
                source_rom.chr().len()
            );
        }
        Command::BuildReleaseImage {
            cumulative,
            output,
            report,
        } => {
            let cumulative_rom = rom::Rom::from_path(&cumulative)?;
            let (image, plan) = release_image::build_release_image(&cumulative_rom)?;
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&output, &image)?;
            let json = serde_json::to_string_pretty(&plan)?;
            std::fs::write(&report, format!("{json}\n"))?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", plan.output_sha1);
            println!(
                "release image: {} header, mapper {}, PRG {} bytes, CHR {} -> {} bytes ({} zero pages appended), declares {} bytes CHR RAM and {} bytes battery work RAM",
                plan.header_format,
                plan.mapper,
                plan.prg_byte_count,
                plan.input_chr_byte_count,
                plan.output_chr_byte_count,
                plan.appended_zero_chr_page_count,
                plan.chr_ram_byte_count,
                plan.battery_work_ram_byte_count
            );
        }
        Command::AnalyzeFontSupply {
            source,
            report,
            sheet,
            scale,
        } => {
            let summary = chr_inventory::analyze_font_supply(&source, &report, &sheet, scale)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("wrote {}", sheet.display());
            println!(
                "CHR pages: {}, protected font codes: {}, unresolved font codes: {}",
                summary.page_count, summary.protected_code_count, summary.unresolved_code_count
            );
        }
        Command::AnalyzeTextTables { source, report } => {
            let summary = text_inventory::analyze_text_tables(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "text tables: {}, pointers: {}, unique strings: {}, protected original bytes: {}",
                summary.table_count,
                summary.pointer_count,
                summary.unique_string_count,
                summary.referenced_protected_original_byte_count
            );
        }
        Command::ExtractFixedTextWorkspace { source, output } => {
            let summary = text_inventory::extract_fixed_text_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "fixed text: {} unique entries, {} Japanese-bearing, {} translations preserved",
                summary.entry_count,
                summary.japanese_entry_count,
                summary.preserved_translation_count
            );
        }
        Command::ExtractFrontEndMenuWorkspace { source, output } => {
            let summary = front_end_menu::extract_front_end_menu_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "front-end menu: {} entries, {} translations preserved",
                summary.entry_count, summary.preserved_translation_count
            );
        }
        Command::ExtractUnitNameWorkspace { source, output } => {
            let summary = text_inventory::extract_unit_name_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "unit names: {} entries, {} Japanese entries, {} translations preserved",
                summary.entry_count,
                summary.japanese_entry_count,
                summary.preserved_translation_count
            );
        }
        Command::ExtractLocationNameWorkspace { source, output } => {
            let summary = text_inventory::extract_location_name_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "location names: {} entries, {} Japanese entries, {} translations preserved",
                summary.entry_count,
                summary.japanese_entry_count,
                summary.preserved_translation_count
            );
        }
        Command::ExtractClassProfileWorkspace { source, output } => {
            let summary = class_profile::extract_class_profile_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "class profiles: {} entries, {} description lines, {} translations preserved",
                summary.entry_count,
                summary.description_line_count,
                summary.preserved_translation_count
            );
        }
        Command::AnalyzeBattleTextWorkset {
            source,
            fixed_workspace,
            dialogue_workspace,
            report,
        } => {
            let summary = battle_text_workset::analyze_battle_text_workset(
                &source,
                &fixed_workspace,
                &dialogue_workspace,
                &report,
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle glyphs: fixed {}, dialogue {}, union {}, conservative combination upper bound {}",
                summary.fixed_glyph_count,
                summary.dialogue_glyph_count,
                summary.union_glyph_count,
                summary.conservative_combination_upper_bound
            );
        }
        Command::AnalyzeBattleCodebookPlan {
            source,
            fixed_workspace,
            dialogue_workspace,
            report,
        } => {
            let summary = mapper165::battle_codebook_plan::analyze_battle_codebook_plan(
                &source,
                &fixed_workspace,
                &dialogue_workspace,
                &report,
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle stable codebook: {} glyphs, {} conflicts, clique lower bound {}, coloring upper bound {}, {} chapter-one-safe codes",
                summary.glyph_count,
                summary.conflict_edge_count,
                summary.constructed_clique_glyph_count,
                summary.stable_color_count,
                summary.chapter_one_safe_code_count
            );
        }
        Command::AnalyzeBattleSurfaceConstraints {
            source,
            fixed_workspace,
            dialogue_workspace,
            temporal_manifest,
            report,
        } => {
            let summary = mapper165::battle_codebook_plan::surface_constraints::analyze_battle_surface_constraints(
                &source,
                &fixed_workspace,
                &dialogue_workspace,
                &temporal_manifest,
                &report,
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle surface constraints: {} samples, {} runtime tuples, {:?} constrained colors, physical assignment {:?}",
                summary.sample_count,
                summary.runtime_tuple_count,
                summary.constrained_color_count,
                summary.physical_assignment_sha1
            );
        }
        Command::BuildBattleTextCacheBase {
            source,
            fixed_workspace,
            dialogue_workspace,
            output,
            report,
        } => {
            let summary = mapper165::battle_text_cache_probe::build_battle_text_cache_base(
                &source,
                &fixed_workspace,
                &dialogue_workspace,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle glyph atlas: {} glyphs, {} bytes",
                summary.glyph_count, summary.glyph_atlas_byte_count
            );
        }
        Command::BuildBattleTextRuntimeBase {
            source,
            fixed_workspace,
            dialogue_workspace,
            temporal_manifest,
            output,
            report,
        } => {
            let summary = mapper165::battle_text_runtime_base::build_battle_text_runtime_base(
                &source,
                &fixed_workspace,
                &dialogue_workspace,
                &temporal_manifest,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle runtime text base: {} fixed entries, {} dialogue records, {} tracked text writes",
                summary.fixed_entry_count,
                summary.dialogue_record_count,
                summary.tracked_write_count
            );
        }
        Command::BuildBattleCompositionLoaderProbe {
            source,
            temporal_manifest,
            base,
            base_report,
            output,
            report,
        } => {
            let summary =
                mapper165::battle_composition_loader_probe::build_battle_composition_loader_probe(
                    &source,
                    &temporal_manifest,
                    &base,
                    &base_report,
                    &output,
                    &report,
                )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle composition loader: {} observed verification tuples, at most {} observed PPU data writes, {} runtime bytes",
                summary.observed_runtime_tuple_count,
                summary.maximum_observed_ppu_write_count,
                summary.runtime_routine_byte_count
            );
        }
        Command::VerifyBattleCompositionRuntime { rom, event, report } => {
            let summary =
                mapper165::battle_composition_runtime_verify::verify_battle_composition_runtime(
                    &rom, &event, &report,
                )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle runtime composition: expected {}, actual {}, {} differing bytes across {} tiles",
                summary.expected_chr_ram_sha1,
                summary.actual_chr_ram_sha1,
                summary.differing_byte_count,
                summary.differing_tile_count
            );
        }
        Command::BuildBattleCombinationProbe {
            source,
            fixed_workspace,
            dialogue_workspace,
            output,
            report,
        } => {
            let summary =
                mapper165::battle_combination_probe::build_gameplay_battle_combination_probe(
                    &source,
                    &fixed_workspace,
                    &dialogue_workspace,
                    &output,
                    &report,
                )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "gameplay battle combination: {} glyphs, {} tracked writes",
                summary.glyph_count, summary.tracked_write_count
            );
        }
        Command::BuildBattleCacheUploadProbe {
            source,
            fixed_workspace,
            dialogue_workspace,
            output,
            report,
        } => {
            let summary = mapper165::battle_cache_upload_probe::build_battle_cache_upload_probe(
                &source,
                &fixed_workspace,
                &dialogue_workspace,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle cache upload: {} glyphs, {} runtime writes",
                summary.glyph_count, summary.runtime_tracked_write_count
            );
        }
        Command::AnalyzeDialogueStructure { source, report } => {
            let summary = dialogue_inventory::analyze_dialogue_structure(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "dialogue tables: {}, pointers: {}, unique targets: {}, alias groups: {}",
                summary.table_count,
                summary.pointer_count,
                summary.unique_target_count,
                summary.alias_group_count
            );
        }
        Command::AnalyzeScreenContracts { source, report } => {
            let summary = screen_contracts::analyze_screen_contracts(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "screen contracts: {}, runtime observed: {}, mixed original-Latin: {}, next observation gate: {}",
                summary.screen_count,
                summary.runtime_observed_screen_count,
                summary.mixed_original_latin_screen_count,
                summary.next_observation_gate_role
            );
        }
        Command::AnalyzeTranslationCoverage {
            source,
            main_dialogue_workspace,
            battle_dialogue_workspace,
            fixed_text_workspace,
            options_localization,
            roster_localization,
            front_end_menu_localization,
            unit_name_localization,
            class_profile_localization,
            chapter_title_localization,
            choice_label_localization,
            map_menu_localization,
            title_graphics_localization,
            unit_ui_label_localization,
            item_action_label_localization,
            transition_label_localization,
            chapter_save_continue_prompt_manifest,
            location_name_localization,
            current_build_output,
            current_build_report,
            main_dialogue_glyph_workset_report,
            battle_surface_constraints_report,
            unit_ui_text_report,
            report,
        } => {
            let summary = translation_coverage::analyze_translation_coverage(
                translation_coverage::TranslationCoverageInputs {
                    source_path: &source,
                    main_dialogue_workspace_path: &main_dialogue_workspace,
                    battle_dialogue_workspace_path: &battle_dialogue_workspace,
                    fixed_text_workspace_path: &fixed_text_workspace,
                    options_localization_path: &options_localization,
                    roster_localization_path: &roster_localization,
                    front_end_menu_localization_path: &front_end_menu_localization,
                    unit_name_localization_path: &unit_name_localization,
                    class_profile_localization_path: &class_profile_localization,
                    chapter_title_localization_path: &chapter_title_localization,
                    choice_label_localization_path: &choice_label_localization,
                    map_menu_localization_path: &map_menu_localization,
                    title_graphics_localization_path: &title_graphics_localization,
                    unit_ui_label_localization_path: &unit_ui_label_localization,
                    item_action_label_localization_path: &item_action_label_localization,
                    transition_label_localization_path: &transition_label_localization,
                    chapter_save_continue_prompt_manifest_path:
                        &chapter_save_continue_prompt_manifest,
                    location_name_localization_path: &location_name_localization,
                    current_build_output_path: &current_build_output,
                    current_build_report_path: &current_build_report,
                    main_dialogue_glyph_workset_report_path: &main_dialogue_glyph_workset_report,
                    battle_surface_constraints_report_path: &battle_surface_constraints_report,
                    unit_ui_text_report_path: &unit_ui_text_report,
                    report_path: &report,
                },
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "translation coverage: {} Japanese-bearing screens, {} domains, {} unresolved source domains, {} domains installed for all consumers",
                summary.japanese_bearing_screen_count,
                summary.domain_count,
                summary.unresolved_source_domain_count,
                summary.all_consumers_installed_domain_count
            );
        }
        Command::PlanFullTranslationInstallation {
            source,
            main_dialogue_workspace,
            fixed_text_workspace,
            unit_name_localization,
            chapter_title_localization,
            choice_label_localization,
            map_menu_localization,
            unit_ui_label_localization,
            item_action_label_localization,
            transition_label_localization,
            location_name_localization,
            current_candidate,
            current_build_report,
            report,
        } => {
            let summary = full_translation_install::plan_full_translation_installation(
                full_translation_install::FullTranslationInstallInputs {
                    source_path: &source,
                    main_dialogue_workspace_path: &main_dialogue_workspace,
                    fixed_text_workspace_path: &fixed_text_workspace,
                    unit_name_localization_path: &unit_name_localization,
                    chapter_title_localization_path: &chapter_title_localization,
                    choice_label_localization_path: &choice_label_localization,
                    map_menu_localization_path: &map_menu_localization,
                    unit_ui_label_localization_path: &unit_ui_label_localization,
                    item_action_label_localization_path: &item_action_label_localization,
                    transition_label_localization_path: &transition_label_localization,
                    location_name_localization_path: &location_name_localization,
                    current_candidate_path: &current_candidate,
                    current_build_report_path: &current_build_report,
                    report_path: &report,
                },
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "full translation installation: {} required domains, {} dialogue records, {} page worksets, {} glyphs with a {}-page static upper bound and maximum {}-slot page demand, {} pointer writes, {} planned bytes",
                summary.required_domain_count,
                summary.dialogue_record_count,
                summary.dialogue_page_workset_count,
                summary.dialogue_glyph_count,
                summary.dialogue_static_page_upper_bound_count,
                summary.dialogue_maximum_page_slot_demand,
                summary.dialogue_pointer_write_count,
                summary.dialogue_planned_storage_byte_count,
            );
        }
        Command::AnalyzeChapterTransitions { source, report } => {
            let summary = chapter_transition::analyze_chapter_transitions(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "chapter transitions: {} observed screens, {} chapter contexts, {} chapter titles, {} intro runtime samples, {} source-bound regions, next observation gate: {}",
                summary.screen_count,
                summary.chapter_context_count,
                summary.chapter_title_count,
                summary.chapter_intro_runtime_sample_count,
                summary.source_region_count,
                summary.next_observation_gate_role
            );
        }
        Command::ExtractChapterTitleWorkspace { source, output } => {
            let summary = chapter_transition::extract_chapter_title_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "chapter titles: {} entries, {} Japanese-bearing, {} translations preserved",
                summary.entry_count,
                summary.japanese_entry_count,
                summary.preserved_translation_count
            );
        }
        Command::AnalyzeTemporalSurfaces {
            source,
            manifest,
            report,
        } => {
            let summary = temporal_surface::analyze_temporal_surfaces(&source, &manifest, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "temporal surfaces: {} routes, {} samples, {} CHR pairs, required route coverage complete: {}",
                summary.route_count,
                summary.sample_count,
                summary.chr_pair_count,
                summary.required_route_coverage_complete
            );
        }
        Command::AnalyzeEpilogueVariants {
            source,
            captures,
            capture_rom,
            mapper_report,
            report,
        } => {
            let summary = epilogue_variant_evidence::analyze_epilogue_variants(
                &source,
                &capture_rom,
                &mapper_report,
                &captures,
                &report,
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "epilogue variants: {} visible entries, {} irregular samples, {} CHR pairs, evidence complete: {}",
                summary.visible_entry_count,
                summary.sample_count,
                summary.chr_pair_count,
                summary.evidence_complete
            );
        }
        Command::AnalyzeChapterVictory { source, report } => {
            let summary = chapter_victory::analyze_chapter_victory(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "chapter victory route: {} source castle tiles, {} source-bound regions, {} route steps, {} runtime screens, continuous gate closed: {}, next observation gate: {}",
                summary.victory_tile_count,
                summary.source_region_count,
                summary.route_step_count,
                summary.runtime_screen_count,
                summary.continuous_gate_closed,
                summary.next_observation_gate
            );
        }
        Command::AnalyzeShopFlow { source, report } => {
            let summary = shop_flow::analyze_shop_flow(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "shop flow: {} observed screens, {} source-bound regions, next: {}",
                summary.screen_count, summary.source_region_count, summary.next_screen_role
            );
        }
        Command::AnalyzeItemFlow { source, report } => {
            let summary = item_flow::analyze_item_flow(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "item flow: {} screen roles, {} source-bound regions, {} action choices, {} usable items, next: {}",
                summary.screen_count,
                summary.source_region_count,
                summary.action_count,
                summary.usable_item_count,
                summary.next_screen_role
            );
        }
        Command::AnalyzeUnitUiText {
            source,
            fixed_text_workspace,
            unit_name_localization,
            unit_ui_label_localization,
            report,
        } => {
            let summary = unit_ui_text::analyze_unit_ui_text(
                &source,
                &fixed_text_workspace,
                &unit_name_localization,
                &unit_ui_label_localization,
                &report,
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "unit UI text: {} screen roles, {} composers, {} fixed labels, {} Japanese labels targeted, {} command labels, {} dynamic pointers / {} unique strings, Hangul ceiling {}, single-family fit {}",
                summary.screen_role_count,
                summary.composer_count,
                summary.fixed_label_count,
                summary.translated_japanese_label_count,
                summary.command_label_count,
                summary.dynamic_pointer_count,
                summary.dynamic_unique_string_count,
                summary.provisional_hangul_slot_ceiling,
                summary.single_family_page_fit
            );
        }
        Command::ExtractMainDialogueSource { source, output } => {
            let summary = dialogue_assets::extract_main_dialogue_source(&source, &output)?;
            println!("wrote {}", output.display());
            println!("asset SHA-1: {}", summary.asset_sha1);
            println!(
                "main dialogue source: {} regions, {} records, {} unique storage bytes",
                summary.storage_region_count,
                summary.record_count,
                summary.unique_storage_byte_count
            );
        }
        Command::ExtractMainDialogueWorkspace { source, output } => {
            let summary = dialogue_assets::extract_main_dialogue_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "main dialogue workspace: {} records, {} lines, {} safe Japanese source bytes, {} relocation-blocked lines, {} preserved translations",
                summary.record_count,
                summary.line_count,
                summary.safe_japanese_source_byte_count,
                summary.blocked_line_count,
                summary.preserved_translation_line_count
            );
        }
        Command::ExtractBattleDialogueWorkspace { source, output } => {
            let summary = dialogue_assets::extract_battle_dialogue_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "battle dialogue workspace: {} records, {} lines, {} Japanese source bytes, {} preserved translations",
                summary.record_count,
                summary.line_count,
                summary.japanese_source_byte_count,
                summary.preserved_translation_line_count
            );
        }
        Command::ValidateBattleDialogueWorkspace { source, workspace } => {
            let summary = dialogue_assets::validate_battle_dialogue_workspace(&source, &workspace)?;
            println!("validated {}", workspace.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "battle dialogue translations: {} records, {} lines, {} filled, {} complete, {} target glyphs, {} translated-record bytes + {} preserved bytes = {} planned bytes, {} bytes remaining",
                summary.record_count,
                summary.line_count,
                summary.filled_line_count,
                summary.complete_line_count,
                summary.target_glyph_count,
                summary.translated_record_storage_byte_count,
                summary.preserved_unreferenced_storage_byte_count,
                summary.planned_storage_byte_count,
                summary.remaining_storage_byte_count
            );
        }
        Command::ImportBattleDialogueDraft { workspace, draft } => {
            let summary = dialogue_assets::import_battle_dialogue_draft(&workspace, &draft)?;
            println!("updated {}", workspace.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "imported {} battle dialogue draft lines",
                summary.imported_line_count
            );
        }
        Command::PlanBattleDialogueReinsertion {
            source,
            workspace,
            report,
        } => {
            let summary =
                dialogue_assets::plan_battle_dialogue_reinsertion(&source, &workspace, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle dialogue layout: {} records, {} pointer writes, {} translated bytes + {} preserved bytes, {} bytes remaining",
                summary.record_count,
                summary.pointer_write_count,
                summary.translated_record_storage_byte_count,
                summary.preserved_storage_byte_count,
                summary.remaining_storage_byte_count
            );
        }
        Command::BuildBattleDialogueProbe {
            source,
            workspace,
            output,
            report,
        } => {
            let summary = mapper165::battle_dialogue_probe::build_battle_dialogue_probe(
                &source, &workspace, &output, &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle dialogue probe: {} records, {} translated lines, {} pointer writes, {} unique glyphs, {} tracked writes",
                summary.record_count,
                summary.translated_line_count,
                summary.pointer_write_count,
                summary.unique_glyph_count,
                summary.tracked_write_count
            );
        }
        Command::ValidateMainDialogueWorkspace { source, workspace } => {
            let summary = dialogue_assets::validate_main_dialogue_workspace(&source, &workspace)?;
            println!("validated {}", workspace.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "main dialogue translations: {} records, {} lines, {} filled, {} complete, {} source-preserved, {} untranslated Japanese, {} target glyphs, input complete: {}, review complete: {}",
                summary.record_count,
                summary.line_count,
                summary.filled_line_count,
                summary.complete_line_count,
                summary.preserved_source_line_count,
                summary.untranslated_japanese_line_count,
                summary.target_glyph_count,
                summary.translation_input_complete,
                summary.review_complete
            );
        }
        Command::AnalyzeMainDialogueGlyphWorkset {
            source,
            workspace,
            maximum_lifetime_evidence,
            report,
        } => {
            let summary = dialogue_assets::analyze_main_dialogue_glyph_workset(
                &source,
                &workspace,
                &maximum_lifetime_evidence,
                &report,
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "main dialogue glyph workset: {} filled lines, {} complete lines, {} filled unique glyphs, {} approved unique glyphs, max transition chain {} glyphs, chains fit one page: {}, max observed screen lifetime {} slots, observed lifetimes fit one page: {}, draft ready: {}",
                summary.filled_line_count,
                summary.complete_line_count,
                summary.filled_unique_glyph_count,
                summary.approved_unique_glyph_count,
                summary.max_transition_chain_unique_glyph_count,
                summary.filled_transition_chains_fit_one_page,
                summary.max_observed_screen_lifetime_slot_demand,
                summary.filled_observed_screen_lifetimes_fit_one_page,
                summary.working_set_ready
            );
        }
        Command::PlanMainDialogueReinsertion {
            source,
            workspace,
            report,
        } => {
            let summary =
                dialogue_assets::plan_main_dialogue_reinsertion(&source, &workspace, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "main dialogue layout: {} regions, {} records, {} pointer writes, {} planned bytes, {} remaining bytes, {} changed records, translation input complete: {}, review complete: {}, release eligible: {}",
                summary.region_count,
                summary.record_count,
                summary.pointer_write_count,
                summary.planned_storage_byte_count,
                summary.remaining_storage_byte_count,
                summary.changed_record_count,
                summary.translation_input_complete,
                summary.review_complete,
                summary.release_eligible
            );
        }
        Command::VerifyMainDialogueSourceRoundtrip {
            source,
            asset,
            output,
        } => {
            let summary =
                dialogue_assets::verify_main_dialogue_source_roundtrip(&source, &asset, &output)?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!(
                "verified exact source roundtrip: {} regions, {} records",
                summary.storage_region_count, summary.record_count
            );
        }
        Command::BuildTitleLogoAsset {
            source,
            manifest,
            asset,
            preview,
            report,
        } => {
            let summary = title_graphics::build_title_logo_asset(
                &source, &manifest, &asset, &preview, &report,
            )?;
            println!("wrote {}", asset.display());
            println!("asset SHA-1: {}", summary.asset_sha1);
            println!("wrote {}", preview.display());
            println!("preview SHA-1: {}", summary.preview_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "title logo: {} unique target tiles in {} source-owned slots",
                summary.target_unique_nonblank_tile_count, summary.source_owned_tile_count
            );
        }
        Command::BuildOptionsPoc {
            source,
            localization,
            output,
            preview,
            preview_scale,
        } => {
            let report = options::build_options_poc(
                &source,
                &localization,
                &output,
                &preview,
                preview_scale,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", report.output_sha1);
            println!("wrote {}", preview.display());
            for write in report.writes {
                println!(
                    "tracked write: {} at {:#08X} ({} bytes)",
                    write.label, write.offset, write.len
                );
            }
        }
        Command::BuildMapper165ParityProbe {
            source,
            output,
            report,
        } => {
            let summary = mapper165::build_mapper165_parity_probe(&source, &output, &report)?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("tracked ROM writes: {}", summary.tracked_write_count);
        }
        Command::PlanHangulPageProof {
            source,
            localization,
            page_pack,
            report,
        } => {
            let summary = hangul_page_plan::plan_hangul_page_proof(
                &source,
                &localization,
                &page_pack,
                &report,
            )?;
            println!("wrote {}", page_pack.display());
            println!("page pack SHA-1: {}", summary.page_pack_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "Hangul pages: {} active slots, {}-glyph proof union, {} extension pages available",
                summary.active_hangul_slot_count,
                summary.page_union_glyph_count,
                summary.maximum_extension_page_count
            );
        }
        Command::BuildMapper165HangulPageProbe {
            source,
            localization,
            roster_localization,
            options_screen_evidence,
            output,
            report,
        } => {
            let summary = mapper165::hangul_page_probe::build_mapper165_hangul_page_probe(
                &source,
                &localization,
                &roster_localization,
                &options_screen_evidence,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("options page pack SHA-1: {}", summary.page_pack_sha1);
            println!("roster page pack SHA-1: {}", summary.roster_page_pack_sha1);
            println!("tracked ROM writes: {}", summary.tracked_write_count);
        }
        Command::BuildKrPatch {
            source,
            options_localization,
            roster_localization,
            options_screen_evidence,
            main_dialogue_workspace,
            chapter_title_localization,
            front_end_menu_localization,
            unit_name_localization,
            class_profile_localization,
            fixed_text_workspace,
            battle_dialogue_workspace,
            battle_temporal_manifest,
            choice_label_localization,
            chapter_one_intro_evidence,
            chapter_two_intro_evidence,
            front_end_menu_evidence,
            unit_name_evidence,
            class_profile_evidence,
            class_profile_runtime_evidence,
            shop_dialogue_evidence,
            shop_dialogue_runtime_evidence,
            weapon_shop_shared_text_runtime_evidence,
            maximum_dialogue_evidence,
            maximum_dialogue_page_boundaries,
            maximum_dialogue_runtime_evidence,
            title_graphics_localization,
            title_logo_asset,
            title_logo_runtime_evidence,
            stage_directory,
            output,
            report,
        } => {
            let summary = mapper165::cumulative_patch::build_cumulative_patch(
                mapper165::cumulative_patch::CumulativePatchInputs {
                    source_path: &source,
                    options_localization_path: &options_localization,
                    roster_localization_path: &roster_localization,
                    options_screen_evidence_path: &options_screen_evidence,
                    main_dialogue_workspace_path: &main_dialogue_workspace,
                    chapter_title_localization_path: &chapter_title_localization,
                    front_end_menu_localization_path: &front_end_menu_localization,
                    unit_name_localization_path: &unit_name_localization,
                    class_profile_localization_path: &class_profile_localization,
                    fixed_text_workspace_path: &fixed_text_workspace,
                    battle_dialogue_workspace_path: &battle_dialogue_workspace,
                    battle_temporal_manifest_path: &battle_temporal_manifest,
                    choice_label_localization_path: &choice_label_localization,
                    chapter_one_intro_evidence_path: &chapter_one_intro_evidence,
                    chapter_two_intro_evidence_path: &chapter_two_intro_evidence,
                    front_end_menu_evidence_path: &front_end_menu_evidence,
                    unit_name_evidence_path: &unit_name_evidence,
                    class_profile_evidence_path: &class_profile_evidence,
                    class_profile_runtime_evidence_path: &class_profile_runtime_evidence,
                    shop_dialogue_evidence_path: &shop_dialogue_evidence,
                    shop_dialogue_runtime_evidence_path: &shop_dialogue_runtime_evidence,
                    weapon_shop_shared_text_runtime_evidence_path:
                        &weapon_shop_shared_text_runtime_evidence,
                    maximum_dialogue_evidence_path: &maximum_dialogue_evidence,
                    maximum_dialogue_page_boundary_path: &maximum_dialogue_page_boundaries,
                    maximum_dialogue_runtime_evidence_path: maximum_dialogue_runtime_evidence
                        .as_deref(),
                    title_graphics_localization_path: &title_graphics_localization,
                    title_logo_asset_path: &title_logo_asset,
                    title_logo_runtime_evidence_path: &title_logo_runtime_evidence,
                    stage_directory: &stage_directory,
                    output_path: &output,
                    report_path: &report,
                },
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
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
        }
        Command::BuildMainDialogueSliceProbe {
            source,
            workspace,
            screen_evidence,
            record_id,
            output,
            report,
        } => {
            let summary = mapper165::dialogue_slice_probe::build_dialogue_slice_probe(
                &source,
                &workspace,
                &screen_evidence,
                &record_id,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "dialogue slice: {} lines, {} unique glyphs, {} planned bytes, {} bytes remaining, {} preserved active codes, {} temporal samples, {} tracked writes",
                summary.translated_line_count,
                summary.unique_glyph_count,
                summary.planned_storage_byte_count,
                summary.remaining_storage_byte_count,
                summary.preserved_active_code_count,
                summary.temporal_sample_count,
                summary.tracked_write_count
            );
        }
        Command::AnalyzeMapper165TriggerPlanes { source, report } => {
            let summary =
                mapper165::trigger_planes::analyze_mapper165_trigger_planes(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("observed screens: {}", summary.observed_screen_count);
            println!("unique FD/FE pairs: {}", summary.unique_pair_count);
            println!(
                "required CHR variant pages: {}",
                summary.required_variant_page_count
            );
            println!(
                "pair-aware selector required: {}",
                summary.pair_aware_selector_required
            );
        }
        Command::AnalyzeMapper165DirectChrPairs { source, report } => {
            let summary = mapper165::direct_chr_pairs::analyze_direct_chr_pairs(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("direct CHR writers: {}", summary.direct_writer_count);
            println!(
                "same-value paired writers: {}",
                summary.same_value_writer_count
            );
            println!(
                "runtime-value or singleton writers: {}",
                summary.runtime_observation_writer_count
            );
        }
        Command::BuildMmc5PrgProbe {
            source,
            output,
            report,
        } => {
            let summary = mmc5_prg::build_mmc5_prg_probe(&source, &output, &report)?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("tracked ROM writes: {}", summary.tracked_write_count);
        }
        Command::BuildMmc5ChrWriterProbe {
            source,
            output,
            report,
        } => {
            let summary = mmc5_chr::build_mmc5_chr_writer_probe(&source, &output, &report)?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "tracked writes after PRG probe: {}",
                summary.tracked_delta_write_count
            );
        }
        Command::BuildMmc5ExpandedChrOptionsProbe {
            source,
            localization,
            output,
            report,
        } => {
            let summary = mmc5_expanded_chr::build_mmc5_expanded_chr_options_probe(
                &source,
                &localization,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("tracked ROM writes: {}", summary.tracked_write_count);
        }
        Command::BuildMmc5DialogueExramProbe {
            source,
            attributes,
            output,
            report,
        } => {
            let summary = mmc5_exram_probe::build_mmc5_dialogue_exram_probe(
                &source,
                &attributes,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "tracked writes after CHR writer probe: {}",
                summary.tracked_write_count
            );
        }
        Command::BuildMmc5NametableShadowProbe {
            source,
            output,
            report,
        } => {
            let summary = mmc5_nametable_shadow::build_mmc5_nametable_shadow_probe(
                &source, &output, &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "hooked direct PPU stores: {}, tracked writes after CHR writer probe: {}",
                summary.hooked_store_count, summary.tracked_write_count
            );
        }
        Command::BuildMmc5QueueShadowProbe {
            source,
            output,
            report,
        } => {
            let summary =
                mmc5_queue_shadow::build_mmc5_queue_shadow_probe(&source, &output, &report)?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "tracked writes after CHR writer probe: {}",
                summary.tracked_write_count
            );
        }
        Command::ProjectMmc4LatchNametable {
            input,
            nametable_index,
            fd_bank,
            fe_bank,
            initial_latch,
            output,
            report,
        } => {
            let summary = mmc4_latch::project_mmc4_latch_nametable(
                &input,
                nametable_index,
                fd_bank,
                fe_bank,
                initial_latch,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "MMC4 latch triggers: FD {}, FE {}, ending latch {}",
                summary.fd_trigger_count, summary.fe_trigger_count, summary.ending_latch
            );
        }
        Command::ReplayMmc4LatchPpuTransfers {
            input,
            output,
            report,
        } => {
            let summary = mmc4_latch::replay_mmc4_latch_ppu_transfers(&input, &output, &report)?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "applied nametable writes: {}",
                summary.nametable_write_count
            );
        }
    }
    Ok(())
}

pub(crate) fn sha1_hex(data: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    format!("{:x}", Sha1::digest(data))
}
