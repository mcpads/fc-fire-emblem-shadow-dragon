mod chr_inventory;
mod font;
mod localization;
mod options;
mod rom;
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
    }
    Ok(())
}

pub(crate) fn sha1_hex(data: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    format!("{:x}", Sha1::digest(data))
}
