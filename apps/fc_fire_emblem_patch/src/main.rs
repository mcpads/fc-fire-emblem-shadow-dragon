mod chr_inventory;
mod dialogue_assets;
mod dialogue_inventory;
mod font;
mod japanese_encoding;
mod localization;
mod mmc5_prg;
mod options;
mod rom;
mod rp2a03;
mod static_analysis;
mod text_inventory;
mod tracked;

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
    /// Build the static MMC5 PRG and SRAM conversion probe without translation assets.
    BuildMmc5PrgProbe {
        source: PathBuf,
        #[arg(long, default_value = "out/fire-emblem-fe1-mmc5-prg-probe.nes")]
        output: PathBuf,
        #[arg(long, default_value = "out/mmc5-prg-probe.json")]
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
    }
    Ok(())
}

pub(crate) fn sha1_hex(data: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    format!("{:x}", Sha1::digest(data))
}
