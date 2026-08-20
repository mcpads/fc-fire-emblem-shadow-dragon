mod battle_text_workset;
mod chapter_map_source;
mod chapter_transition;
mod chapter_victory;
mod choice_labels;
mod chr_inventory;
mod class_profile;
mod command_line;
mod dialogue_assets;
mod dialogue_inventory;
mod epilogue_variant_evidence;
mod fixed_menu_labels;
mod fixed_string_consumers;
mod fixed_string_ownership;
mod font;
mod font_slots;
mod front_end_menu;
mod full_translation_install;
mod glyph_demand;
mod hangul_page_plan;
mod item_flow;
mod japanese_encoding;
mod localization;
mod map_dialogue_lifecycle;
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
mod runtime_storage_layout;
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
mod translation_consumer;
mod translation_coverage;
mod typed_source;
mod unit_names;
mod unit_ui_text;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use command_line::{
    AnalyzeTranslationCoverageCommand, BuildCumulativePatchCommand,
    PlanFullTranslationInstallationCommand,
};

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
    AnalyzeTranslationCoverage(AnalyzeTranslationCoverageCommand),
    /// Plan every translation domain together and optionally emit the gated integrated ROM.
    PlanFullTranslationInstallation(PlanFullTranslationInstallationCommand),
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
    /// Count per-glyph demand in every translation population and check co-resident sets.
    AnalyzeGlyphDemand {
        #[arg(long, default_value = "private/dialogue/main-workspace.json")]
        main_dialogue_workspace: PathBuf,
        #[arg(long, default_value = "private/fixed-text/battle-workspace.json")]
        fixed_text_workspace: PathBuf,
        /// NAME=UNIT_ID[,UNIT_ID...] population that ignores table boundaries.
        #[arg(long = "population")]
        populations: Vec<String>,
        /// NAME=POPULATION[,POPULATION...] set that must fit one font page together.
        #[arg(long = "coresident")]
        coresident_sets: Vec<String>,
        #[arg(long, default_value_t = font_slots::ACTIVE_HANGUL_SLOT_COUNT)]
        slot_budget: usize,
        #[arg(long, default_value = "out/glyph-demand.json")]
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
    /// Hash one dialogue record's page-boundary-relevant input without emitting its text.
    SummarizeMainDialoguePageBoundaryTopology {
        source: PathBuf,
        #[arg(long, default_value = "private/dialogue/main-workspace.json")]
        workspace: PathBuf,
        #[arg(long, default_value = "village-and-outro-dialogue:024")]
        record_id: String,
    },
    /// Prove reference topology and current rendering for one bound maximum-dialogue record.
    VerifyMaximumDialogueBoundaryRebinding {
        source: PathBuf,
        reference_output: PathBuf,
        candidate_output: PathBuf,
        #[arg(long, default_value = "private/dialogue/main-workspace.json")]
        workspace: PathBuf,
        #[arg(
            long,
            default_value = "evidence/private/chapter7-maximum-installed/page-boundaries.json"
        )]
        page_boundaries: PathBuf,
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
    BuildKrPatch(BuildCumulativePatchCommand),
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
    command_line::execute(Cli::parse().command)
}

pub(crate) fn sha1_hex(data: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    format!("{:x}", Sha1::digest(data))
}
