use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use sha1::{Digest, Sha1};

use crate::{
    rom::EXPECTED_SOURCE_SHA1,
    sha1_hex,
    temporal_surface::capture_state::{CaptureState, parse_capture_state},
};

const NAMETABLE_BYTE_COUNT: usize = 0x800;
const PRG_RAM_BYTE_COUNT: usize = 0x2000;
const INTERNAL_RAM_BYTE_COUNT: usize = 0x800;
const PALETTE_BYTE_COUNT: usize = 0x20;
const OAM_BYTE_COUNT: usize = 0x100;
const SELECTOR_PRG_RAM_OFFSET: usize = 0x17F4;
const ENTRY_PRG_RAM_OFFSET: usize = 0x17F1;
const PHASE_PRG_RAM_OFFSET: usize = 0x1731;
const SAMPLE_FILES: [&str; 8] = [
    "iram.bin",
    "nametable.bin",
    "oam.bin",
    "palette.bin",
    "prgram.bin",
    "regions.json",
    "screenshot.png",
    "state.json",
];

pub(super) struct CaptureBinding {
    pub(super) capture_rom_sha1: String,
    pub(super) mapper_report_sha1: String,
}

pub(super) struct CaptureFiles {
    pub(super) screenshot: Vec<u8>,
    pub(super) state: CaptureState,
    pub(super) prg_ram: Vec<u8>,
    pub(super) nametable: Vec<u8>,
    pub(super) oam: Vec<u8>,
    pub(super) palette: Vec<u8>,
}

impl CaptureFiles {
    pub(super) fn selector_entry(&self) -> (u8, u8) {
        (
            self.prg_ram[SELECTOR_PRG_RAM_OFFSET],
            self.prg_ram[ENTRY_PRG_RAM_OFFSET],
        )
    }

    pub(super) fn phase(&self) -> u8 {
        self.prg_ram[PHASE_PRG_RAM_OFFSET]
    }
}

#[derive(Debug, Deserialize)]
struct MapperReportBinding {
    schema: u8,
    source_sha1: String,
    output_sha1: String,
}

pub(super) fn validate_capture_binding(
    capture_rom_path: &Path,
    mapper_report_path: &Path,
) -> Result<CaptureBinding> {
    let capture_rom = fs::read(capture_rom_path)
        .with_context(|| format!("read capture ROM {}", capture_rom_path.display()))?;
    let capture_rom_sha1 = sha1_hex(&capture_rom);
    let mapper_report = fs::read(mapper_report_path)
        .with_context(|| format!("read mapper report {}", mapper_report_path.display()))?;
    let binding: MapperReportBinding = serde_json::from_slice(&mapper_report)
        .with_context(|| format!("parse mapper report {}", mapper_report_path.display()))?;
    ensure!(
        binding.schema == 2,
        "unsupported mapper report schema {}",
        binding.schema
    );
    ensure!(
        binding
            .source_sha1
            .eq_ignore_ascii_case(EXPECTED_SOURCE_SHA1),
        "mapper report source SHA-1 does not match the supported Japanese source"
    );
    ensure!(
        binding.output_sha1.eq_ignore_ascii_case(&capture_rom_sha1),
        "capture ROM SHA-1 does not match the mapper report"
    );
    Ok(CaptureBinding {
        capture_rom_sha1,
        mapper_report_sha1: sha1_hex(&mapper_report),
    })
}

pub(super) fn validate_evidence_root(root: &Path) -> Result<()> {
    let expected = BTreeSet::from([
        "all-direct".to_owned(),
        "all-direct-selector-base.mss".to_owned(),
        "all-routing".to_owned(),
        "all-routing-selector-base.mss".to_owned(),
        "natural".to_owned(),
        "selector-base.mss".to_owned(),
    ]);
    let actual = directory_names(root)?;
    ensure!(
        actual == expected,
        "epilogue evidence root has unexpected or missing entries"
    );
    Ok(())
}

pub(super) fn read_capture(path: &Path) -> Result<CaptureFiles> {
    let expected = SAMPLE_FILES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<BTreeSet<_>>();
    ensure!(
        directory_names(path)? == expected,
        "capture {} has unexpected or missing files",
        path.display()
    );

    let screenshot = read(path.join("screenshot.png"))?;
    ensure!(
        screenshot.starts_with(b"\x89PNG\r\n\x1a\n"),
        "capture {} screenshot is not PNG",
        path.display()
    );
    let state_bytes = read(path.join("state.json"))?;
    let state = parse_capture_state(&state_bytes)
        .with_context(|| format!("parse capture state in {}", path.display()))?;
    let prg_ram = read_exact(path.join("prgram.bin"), PRG_RAM_BYTE_COUNT)?;
    let _internal_ram = read_exact(path.join("iram.bin"), INTERNAL_RAM_BYTE_COUNT)?;
    let nametable = read_exact(path.join("nametable.bin"), NAMETABLE_BYTE_COUNT)?;
    let palette = read_exact(path.join("palette.bin"), PALETTE_BYTE_COUNT)?;
    let oam = read_exact(path.join("oam.bin"), OAM_BYTE_COUNT)?;
    let regions = read(path.join("regions.json"))?;
    let _: serde_json::Value = serde_json::from_slice(&regions)
        .with_context(|| format!("parse regions.json in {}", path.display()))?;

    Ok(CaptureFiles {
        screenshot,
        state,
        prg_ram,
        nametable,
        oam,
        palette,
    })
}

pub(super) fn directory_names(path: &Path) -> Result<BTreeSet<String>> {
    let entries = fs::read_dir(path)
        .with_context(|| format!("read evidence directory {}", path.display()))?;
    entries
        .map(|entry| {
            let entry = entry.with_context(|| format!("read entry in {}", path.display()))?;
            entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("non-UTF-8 evidence entry in {}", path.display()))
        })
        .collect()
}

pub(super) fn evidence_tree_sha1(root: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect_regular_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut digest = Sha1::new();
    for (relative, path) in files {
        let bytes = read(path)?;
        digest.update(relative.as_bytes());
        digest.update([0]);
        digest.update(sha1_hex(&bytes).as_bytes());
        digest.update([0]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn collect_regular_files(
    root: &Path,
    current: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<()> {
    for entry in fs::read_dir(current)
        .with_context(|| format!("walk evidence directory {}", current.display()))?
    {
        let entry = entry.with_context(|| format!("read entry in {}", current.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("read type for {}", entry.path().display()))?;
        if file_type.is_dir() {
            collect_regular_files(root, &entry.path(), files)?;
        } else if file_type.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .expect("walked path stays under evidence root")
                .to_string_lossy()
                .replace('\\', "/");
            files.push((relative, entry.path()));
        } else {
            bail!(
                "evidence tree contains a non-regular entry: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn read(path: PathBuf) -> Result<Vec<u8>> {
    fs::read(&path).with_context(|| format!("read {}", path.display()))
}

fn read_exact(path: PathBuf, expected_len: usize) -> Result<Vec<u8>> {
    let bytes = read(path.clone())?;
    ensure!(
        bytes.len() == expected_len,
        "{} has {} bytes; expected {}",
        path.display(),
        bytes.len(),
        expected_len
    );
    Ok(bytes)
}
