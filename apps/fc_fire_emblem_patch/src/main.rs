mod chr_inventory;
mod dialogue_assets;
mod dialogue_inventory;
mod font;
mod font_slots;
mod hangul_page_plan;
mod japanese_encoding;
mod localization;
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
mod rom;
mod roster_localization;
mod rp2a03;
mod screen_contracts;
mod static_analysis;
mod text_inventory;
mod tracked;
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

#[derive(Debug, Subcommand)]
enum Command {
    /// Verify that a ROM is the exact supported Japanese source revision.
    VerifySource { source: PathBuf },
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
    /// Bind unit-summary and unit-status composers, sources, labels, and shared page lifetime.
    AnalyzeUnitUiText {
        source: PathBuf,
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
    /// Validate private translations without encoding or writing a ROM.
    ValidateMainDialogueWorkspace {
        source: PathBuf,
        #[arg(long, default_value = "private/dialogue/main-workspace.json")]
        workspace: PathBuf,
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
            default_value = "out/fire-emblem-fe1-mapper165-hangul-page-probe.nes"
        )]
        output: PathBuf,
        #[arg(long, default_value = "out/mapper165-hangul-page-probe.json")]
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
                "screen contracts: {}, runtime observed: {}, mixed original-Latin: {}, next: {}",
                summary.screen_count,
                summary.runtime_observed_screen_count,
                summary.mixed_original_latin_screen_count,
                summary.next_screen_role
            );
        }
        Command::AnalyzeUnitUiText { source, report } => {
            let summary = unit_ui_text::analyze_unit_ui_text(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "unit UI text: {} screen roles, {} composers, {} fixed labels, {} Japanese labels targeted, {} command labels, {} dynamic pointers / {} unique strings, provisional Hangul ceiling {}, family-page fit {}",
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
                "main dialogue workspace: {} records, {} lines, {} safe Japanese source bytes, {} relocation-blocked lines",
                summary.record_count,
                summary.line_count,
                summary.safe_japanese_source_byte_count,
                summary.blocked_line_count
            );
        }
        Command::ValidateMainDialogueWorkspace { source, workspace } => {
            let summary = dialogue_assets::validate_main_dialogue_workspace(&source, &workspace)?;
            println!("validated {}", workspace.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "main dialogue translations: {} records, {} lines, {} filled, {} complete, {} target glyphs",
                summary.record_count,
                summary.line_count,
                summary.filled_line_count,
                summary.complete_line_count,
                summary.target_glyph_count
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
                "main dialogue layout: {} regions, {} records, {} pointer writes, {} planned bytes, {} remaining bytes, {} changed records, translation input complete: {}, release eligible: {}",
                summary.region_count,
                summary.record_count,
                summary.pointer_write_count,
                summary.planned_storage_byte_count,
                summary.remaining_storage_byte_count,
                summary.changed_record_count,
                summary.translation_input_complete,
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
            output,
            report,
        } => {
            let summary = mapper165::hangul_page_probe::build_mapper165_hangul_page_probe(
                &source,
                &localization,
                &roster_localization,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("options page pack SHA-1: {}", summary.page_pack_sha1);
            println!("roster page SHA-1: {}", summary.roster_page_sha1);
            println!("tracked ROM writes: {}", summary.tracked_write_count);
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
