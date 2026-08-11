use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::{font_slots::active_hangul_codes, rom::EXPECTED_SOURCE_SHA1, sha1_hex};

#[derive(Deserialize)]
struct ContinuePromptManifest {
    format_version: u8,
    screen_role: String,
    target_record_id: String,
    source_sha1: String,
    savestate: SavestateBinding,
    capture: CaptureBinding,
    ppu_contract: PpuContract,
    target_text_regions: Vec<TargetTextRegion>,
    samples: Vec<RuntimeSample>,
    unique_nametable_count: usize,
    nametable_sequence_sha1: String,
    preserved_screen_active_codes_hex: Vec<String>,
    preserved_screen_active_code_count: usize,
    proof_boundary: String,
}

#[derive(Deserialize)]
struct SavestateBinding {
    path: String,
    sha1: String,
}

#[derive(Deserialize)]
struct CaptureBinding {
    path: String,
    sha1: String,
}

#[derive(Deserialize)]
struct PpuContract {
    background_pattern_address: u16,
    sprite_pattern_address: u16,
    physical_target_nametable: usize,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct TargetTextRegion {
    role: String,
    row_start: usize,
    row_end_exclusive: usize,
    column_start: usize,
    column_end_exclusive: usize,
}

#[derive(Deserialize)]
struct RuntimeSample {
    frame_offset: u64,
    frame_count: u64,
    nametable_sha1: String,
}

#[derive(Deserialize)]
struct NametableCapture {
    format_version: u8,
    source_sha1: String,
    savestate_sha1: String,
    memory_type: String,
    memory_offset: usize,
    memory_byte_count: usize,
    physical_nametable_byte_count: usize,
    samples: Vec<NametableCaptureSample>,
}

#[derive(Deserialize)]
struct NametableCaptureSample {
    frame_offset: u64,
    frame_count: u64,
    ppu_frame_count: u64,
    background_pattern_address: u16,
    sprite_pattern_address: u16,
    nametable_ram_hex: String,
}

pub(super) struct ContinuePromptEvidence {
    pub(super) manifest_sha1: String,
    pub(super) preserved_screen_active_codes: BTreeSet<u8>,
}

pub(super) fn load(path: &Path) -> Result<ContinuePromptEvidence> {
    let manifest_bytes =
        fs::read(path).with_context(|| format!("read chapter-save runtime {}", path.display()))?;
    let manifest: ContinuePromptManifest = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("parse chapter-save runtime {}", path.display()))?;
    ensure!(
        manifest.format_version == 1
            && manifest.screen_role == "chapter_save_complete_continue_prompt"
            && manifest.target_record_id == "victory-and-defeat-dialogue:000"
            && manifest.source_sha1 == EXPECTED_SOURCE_SHA1,
        "chapter-save runtime identity changed"
    );
    ensure!(
        manifest.ppu_contract.background_pattern_address == 0x1000
            && manifest.ppu_contract.sprite_pattern_address == 0
            && manifest.ppu_contract.physical_target_nametable == 0,
        "chapter-save runtime PPU contract changed"
    );
    ensure!(
        manifest.target_text_regions == expected_target_text_regions(),
        "chapter-save runtime target-cell geometry changed"
    );
    validate_temporal_sample_declarations(&manifest)?;

    let manifest_parent = path
        .parent()
        .context("chapter-save runtime manifest has no parent")?;
    let allowed_root = manifest_parent
        .parent()
        .context("chapter-save runtime manifest has no private evidence root")?
        .canonicalize()
        .context("resolve chapter-save private evidence root")?;
    validate_savestate(&manifest, manifest_parent, &allowed_root)?;
    let capture = load_capture(&manifest, manifest_parent, &allowed_root)?;
    let preserved_screen_active_codes = validate_capture_samples(&manifest, &capture)?;

    Ok(ContinuePromptEvidence {
        manifest_sha1: sha1_hex(&manifest_bytes),
        preserved_screen_active_codes,
    })
}

fn expected_target_text_regions() -> Vec<TargetTextRegion> {
    vec![
        TargetTextRegion {
            role: "main_dialogue_interior".to_owned(),
            row_start: 19,
            row_end_exclusive: 27,
            column_start: 9,
            column_end_exclusive: 29,
        },
        TargetTextRegion {
            role: "yes_choice_text".to_owned(),
            row_start: 20,
            row_end_exclusive: 21,
            column_start: 4,
            column_end_exclusive: 7,
        },
        TargetTextRegion {
            role: "no_choice_text".to_owned(),
            row_start: 22,
            row_end_exclusive: 23,
            column_start: 4,
            column_end_exclusive: 7,
        },
    ]
}

fn validate_temporal_sample_declarations(manifest: &ContinuePromptManifest) -> Result<()> {
    let expected_offsets = [0, 7, 19, 43, 82, 171, 308, 565];
    ensure!(
        manifest
            .samples
            .iter()
            .map(|sample| sample.frame_offset)
            .eq(expected_offsets),
        "chapter-save runtime lost its irregular temporal samples"
    );
    let first_frame = manifest
        .samples
        .first()
        .context("chapter-save runtime has no samples")?
        .frame_count;
    ensure!(
        manifest.samples.iter().all(|sample| {
            sample.frame_count == first_frame + sample.frame_offset
                && is_sha1(&sample.nametable_sha1)
        }),
        "chapter-save runtime frame or nametable binding changed"
    );
    let unique_nametables = manifest
        .samples
        .iter()
        .map(|sample| &sample.nametable_sha1)
        .collect::<BTreeSet<_>>();
    ensure!(
        unique_nametables.len() == manifest.unique_nametable_count
            && is_sha1(&manifest.nametable_sequence_sha1),
        "chapter-save runtime temporal union changed"
    );
    Ok(())
}

fn validate_savestate(
    manifest: &ContinuePromptManifest,
    manifest_parent: &Path,
    allowed_root: &Path,
) -> Result<()> {
    let savestate_path = resolve_private_evidence_path(
        manifest_parent,
        allowed_root,
        &manifest.savestate.path,
        "savestate",
    )?;
    ensure!(
        sha1_hex(&fs::read(&savestate_path).with_context(|| {
            format!("read chapter-save savestate {}", savestate_path.display())
        })?) == manifest.savestate.sha1,
        "chapter-save runtime savestate binding changed"
    );
    Ok(())
}

fn load_capture(
    manifest: &ContinuePromptManifest,
    manifest_parent: &Path,
    allowed_root: &Path,
) -> Result<NametableCapture> {
    let capture_path = resolve_private_evidence_path(
        manifest_parent,
        allowed_root,
        &manifest.capture.path,
        "capture",
    )?;
    let capture_bytes = fs::read(&capture_path)
        .with_context(|| format!("read chapter-save capture {}", capture_path.display()))?;
    ensure!(
        sha1_hex(&capture_bytes) == manifest.capture.sha1,
        "chapter-save runtime capture binding changed"
    );
    let capture: NametableCapture = serde_json::from_slice(&capture_bytes)
        .with_context(|| format!("parse chapter-save capture {}", capture_path.display()))?;
    ensure!(
        capture.format_version == 1
            && capture.source_sha1 == EXPECTED_SOURCE_SHA1
            && capture.savestate_sha1 == manifest.savestate.sha1
            && capture.memory_type == "nesNametableRam"
            && capture.memory_offset == 0
            && capture.memory_byte_count == 2 * 1024
            && capture.physical_nametable_byte_count == 1024
            && capture.samples.len() == manifest.samples.len(),
        "chapter-save runtime capture identity changed"
    );
    Ok(capture)
}

fn resolve_private_evidence_path(
    manifest_parent: &Path,
    allowed_root: &Path,
    relative_path: &str,
    role: &str,
) -> Result<std::path::PathBuf> {
    let relative_path = Path::new(relative_path);
    ensure!(
        !relative_path.is_absolute(),
        "chapter-save runtime {role} path must be relative"
    );
    let resolved = manifest_parent
        .join(relative_path)
        .canonicalize()
        .with_context(|| format!("resolve chapter-save runtime {role}"))?;
    ensure!(
        resolved.starts_with(allowed_root),
        "chapter-save runtime {role} escaped private evidence"
    );
    Ok(resolved)
}

fn validate_capture_samples(
    manifest: &ContinuePromptManifest,
    capture: &NametableCapture,
) -> Result<BTreeSet<u8>> {
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let declared_preserved_screen_active_codes = manifest
        .preserved_screen_active_codes_hex
        .iter()
        .map(|hex| {
            ensure!(
                hex.len() == 2 && hex.chars().all(|character| character.is_ascii_hexdigit()),
                "invalid chapter-save runtime active code {hex:?}"
            );
            u8::from_str_radix(hex, 16).context("parse chapter-save runtime active code")
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let mut captured_sequence = Vec::new();
    let mut captured_nametable_hashes = BTreeSet::new();
    let mut preserved_screen_active_codes = BTreeSet::new();
    for (declared, captured) in manifest.samples.iter().zip(&capture.samples) {
        ensure!(
            captured.frame_offset == declared.frame_offset
                && captured.frame_count == declared.frame_count
                && captured.ppu_frame_count == captured.frame_count
                && captured.background_pattern_address
                    == manifest.ppu_contract.background_pattern_address
                && captured.sprite_pattern_address == manifest.ppu_contract.sprite_pattern_address,
            "chapter-save captured sample state changed"
        );
        let nametable_ram = decode_hex(&captured.nametable_ram_hex)
            .context("decode chapter-save captured nametable RAM")?;
        ensure!(
            nametable_ram.len() == capture.memory_byte_count
                && sha1_hex(&nametable_ram) == declared.nametable_sha1,
            "chapter-save captured nametable bytes changed"
        );
        captured_nametable_hashes.insert(declared.nametable_sha1.as_str());
        captured_sequence.extend_from_slice(&nametable_ram);
        preserved_screen_active_codes.extend(preserved_active_codes_from_nametable(
            &nametable_ram,
            capture.physical_nametable_byte_count,
            manifest.ppu_contract.physical_target_nametable,
            &manifest.target_text_regions,
            &active_codes,
        )?);
    }
    ensure!(
        captured_nametable_hashes.len() == manifest.unique_nametable_count
            && sha1_hex(&captured_sequence) == manifest.nametable_sequence_sha1
            && preserved_screen_active_codes == declared_preserved_screen_active_codes
            && preserved_screen_active_codes.len() == manifest.preserved_screen_active_code_count
            && preserved_screen_active_codes.is_subset(&active_codes)
            && manifest
                .proof_boundary
                .contains("consumer-path source-ROM evidence"),
        "chapter-save runtime preservation evidence changed"
    );
    Ok(preserved_screen_active_codes)
}

fn preserved_active_codes_from_nametable(
    nametable_ram: &[u8],
    physical_nametable_byte_count: usize,
    physical_target_nametable: usize,
    target_text_regions: &[TargetTextRegion],
    active_codes: &BTreeSet<u8>,
) -> Result<BTreeSet<u8>> {
    const TILE_COLUMNS: usize = 32;
    const TILE_ROWS: usize = 30;
    const TILE_BYTE_COUNT: usize = TILE_COLUMNS * TILE_ROWS;
    ensure!(
        physical_nametable_byte_count >= TILE_BYTE_COUNT
            && nametable_ram
                .len()
                .is_multiple_of(physical_nametable_byte_count),
        "chapter-save captured nametable geometry changed"
    );
    let physical_nametable_count = nametable_ram.len() / physical_nametable_byte_count;
    ensure!(
        physical_target_nametable < physical_nametable_count,
        "chapter-save target physical nametable is outside the capture"
    );

    let mut preserved = BTreeSet::new();
    for physical_index in 0..physical_nametable_count {
        let start = physical_index * physical_nametable_byte_count;
        for tile_index in 0..TILE_BYTE_COUNT {
            let row = tile_index / TILE_COLUMNS;
            let column = tile_index % TILE_COLUMNS;
            let target_cell = physical_index == physical_target_nametable
                && target_text_regions.iter().any(|region| {
                    row >= region.row_start
                        && row < region.row_end_exclusive
                        && column >= region.column_start
                        && column < region.column_end_exclusive
                });
            let code = nametable_ram[start + tile_index];
            if !target_cell && active_codes.contains(&code) {
                preserved.insert(code);
            }
        }
    }
    Ok(preserved)
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    ensure!(
        value.len().is_multiple_of(2)
            && value.chars().all(|character| character.is_ascii_hexdigit()),
        "chapter-save capture contains invalid hexadecimal bytes"
    );
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).context("decode chapter-save capture hex pair")?;
            u8::from_str_radix(pair, 16).context("parse chapter-save capture hex pair")
        })
        .collect()
}

fn is_sha1(value: &str) -> bool {
    value.len() == 40 && value.chars().all(|character| character.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_union_excludes_only_target_cells_and_attribute_bytes() {
        let mut nametable_ram = vec![0xFF; 2 * 1024];
        nametable_ram[0] = 0x04;
        nametable_ram[19 * 32 + 9] = 0x05;
        nametable_ram[960] = 0x06;
        nametable_ram[1024] = 0x07;
        let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
        let preserved = preserved_active_codes_from_nametable(
            &nametable_ram,
            1024,
            0,
            &[TargetTextRegion {
                role: "target".to_owned(),
                row_start: 19,
                row_end_exclusive: 20,
                column_start: 9,
                column_end_exclusive: 10,
            }],
            &active_codes,
        )
        .unwrap();

        assert_eq!(preserved, BTreeSet::from([0x04, 0x07]));
    }
}
