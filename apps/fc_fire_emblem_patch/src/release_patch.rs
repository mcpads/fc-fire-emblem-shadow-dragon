//! 지원 원본과 검증된 배포 이미지 사이의 BPS1 배포 패치를 만든다.
//!
//! 생성기는 같은 위치의 동일 바이트를 `SourceRead`, 이동한 원본 구간을
//! `SourceCopy`, 새 바이트만 `TargetRead`로 기록한다. 생성 직후 이 모듈의 엄격한
//! 적용기로 다시 적용하고 목표 이미지와 바이트 단위로 비교한 패치만 파일로
//! 내보낸다.

use std::{fs, path::Path};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;

use crate::{
    release_image,
    rom::{self, Rom},
    sha1_hex,
};

const BPS_MAGIC: &[u8; 4] = b"BPS1";
const BPS_FOOTER_SIZE: usize = 12;
const SOURCE_MATCH_PREFIX_SIZE: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ReleasePatchReport {
    pub(crate) format: &'static str,
    pub(crate) source_sha1: String,
    pub(crate) target_sha1: String,
    pub(crate) patch_sha1: String,
    pub(crate) source_size: usize,
    pub(crate) target_size: usize,
    pub(crate) patch_size: usize,
    pub(crate) source_crc32: String,
    pub(crate) target_crc32: String,
    pub(crate) patch_crc32: String,
    pub(crate) source_read_byte_count: usize,
    pub(crate) source_copy_byte_count: usize,
    pub(crate) target_read_byte_count: usize,
    pub(crate) apply_verified: bool,
}

#[derive(Debug)]
enum PatchAction<'a> {
    SourceRead(usize),
    SourceCopy { source_offset: usize, length: usize },
    TargetRead(&'a [u8]),
}

#[derive(Debug, Default)]
struct PatchActionByteCounts {
    source_read: usize,
    source_copy: usize,
    target_read: usize,
}

impl PatchActionByteCounts {
    fn target_size(&self) -> usize {
        self.source_read + self.source_copy + self.target_read
    }
}

#[derive(Debug)]
struct EncodedReleasePatch {
    bytes: Vec<u8>,
    action_byte_counts: PatchActionByteCounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceMatch {
    source_offset: usize,
    length: usize,
}

#[derive(Debug)]
struct SourceMatchIndex {
    prefix_offsets: Vec<(u64, usize)>,
}

impl SourceMatchIndex {
    fn new(source: &[u8]) -> Self {
        let mut prefix_offsets = source
            .windows(SOURCE_MATCH_PREFIX_SIZE)
            .enumerate()
            .map(|(offset, prefix)| (source_match_key(prefix), offset))
            .collect::<Vec<_>>();
        prefix_offsets.sort_unstable();
        Self { prefix_offsets }
    }

    fn longest_match(
        &self,
        source: &[u8],
        target: &[u8],
        target_offset: usize,
    ) -> Option<SourceMatch> {
        let target_prefix = target.get(target_offset..target_offset + SOURCE_MATCH_PREFIX_SIZE)?;
        let key = source_match_key(target_prefix);
        let candidate_start = self
            .prefix_offsets
            .partition_point(|(candidate_key, _)| *candidate_key < key);
        let candidate_end = self
            .prefix_offsets
            .partition_point(|(candidate_key, _)| *candidate_key <= key);
        let mut best: Option<SourceMatch> = None;

        for &(_, source_offset) in &self.prefix_offsets[candidate_start..candidate_end] {
            let maximum_length = (source.len() - source_offset).min(target.len() - target_offset);
            let best_length = best.map_or(0, |source_match| source_match.length);
            if maximum_length <= best_length {
                continue;
            }
            if best_length > 0
                && source[source_offset + best_length] != target[target_offset + best_length]
            {
                continue;
            }

            let mut length = SOURCE_MATCH_PREFIX_SIZE;
            while length < maximum_length
                && source[source_offset + length] == target[target_offset + length]
            {
                length += 1;
            }
            if length > best_length {
                best = Some(SourceMatch {
                    source_offset,
                    length,
                });
            }
        }
        best
    }
}

fn source_match_key(prefix: &[u8]) -> u64 {
    u64::from_le_bytes(prefix[..SOURCE_MATCH_PREFIX_SIZE].try_into().unwrap())
}

#[derive(Debug)]
struct VerifiedReleasePatch {
    bytes: Vec<u8>,
    report: ReleasePatchReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleasePatchSourceLayout {
    Headerless,
    InesHeadered,
}

pub(crate) fn build_release_patch(
    source_path: &Path,
    release_path: &Path,
    output_path: &Path,
    report_path: &Path,
) -> Result<ReleasePatchReport> {
    let source_bytes = read_supported_release_patch_source(source_path)?;

    let release_bytes = fs::read(release_path)
        .with_context(|| format!("read release image {}", release_path.display()))?;
    release_image::verify_release_image(&release_bytes)?;

    let verified = create_verified_release_patch(&source_bytes, &release_bytes)?;
    let report_bytes =
        serde_json::to_vec_pretty(&verified.report).context("serialize release patch report")?;
    let mut report_document = report_bytes;
    report_document.push(b'\n');

    write_release_patch_artifacts(
        source_path,
        release_path,
        output_path,
        report_path,
        &verified.bytes,
        &report_document,
    )?;
    Ok(verified.report)
}

fn read_supported_release_patch_source(source_path: &Path) -> Result<Vec<u8>> {
    let source_bytes =
        fs::read(source_path).with_context(|| format!("read ROM {}", source_path.display()))?;
    match release_patch_source_layout(source_bytes.len())? {
        ReleasePatchSourceLayout::Headerless => {
            rom::verify_supported_japanese_payload(&source_bytes)?;
        }
        ReleasePatchSourceLayout::InesHeadered => {
            Rom::parse(source_bytes.clone())?.verify_supported_japanese()?;
        }
    }
    Ok(source_bytes)
}

fn release_patch_source_layout(source_size: usize) -> Result<ReleasePatchSourceLayout> {
    match source_size {
        rom::SOURCE_PAYLOAD_SIZE => Ok(ReleasePatchSourceLayout::Headerless),
        source_size if source_size == rom::HEADER_SIZE + rom::SOURCE_PAYLOAD_SIZE => {
            Ok(ReleasePatchSourceLayout::InesHeadered)
        }
        _ => bail!(
            "release patch source must be a {}-byte headerless image or a {}-byte iNES image, found {source_size}",
            rom::SOURCE_PAYLOAD_SIZE,
            rom::HEADER_SIZE + rom::SOURCE_PAYLOAD_SIZE
        ),
    }
}

fn create_verified_release_patch(source: &[u8], target: &[u8]) -> Result<VerifiedReleasePatch> {
    let encoded_patch = encode_release_patch(source, target)?;
    ensure!(
        encoded_patch.action_byte_counts.target_size() == target.len(),
        "BPS action byte counts do not cover the target"
    );
    let applied = apply_release_patch(source, &encoded_patch.bytes)?;
    ensure!(
        applied == target,
        "BPS application verification differs from the release image"
    );
    let footer = decode_footer(&encoded_patch.bytes)?;
    Ok(VerifiedReleasePatch {
        report: ReleasePatchReport {
            format: "BPS1",
            source_sha1: sha1_hex(source),
            target_sha1: sha1_hex(target),
            patch_sha1: sha1_hex(&encoded_patch.bytes),
            source_size: source.len(),
            target_size: target.len(),
            patch_size: encoded_patch.bytes.len(),
            source_crc32: format!("{:08X}", footer.source_crc32),
            target_crc32: format!("{:08X}", footer.target_crc32),
            patch_crc32: format!("{:08X}", footer.patch_crc32),
            source_read_byte_count: encoded_patch.action_byte_counts.source_read,
            source_copy_byte_count: encoded_patch.action_byte_counts.source_copy,
            target_read_byte_count: encoded_patch.action_byte_counts.target_read,
            apply_verified: true,
        },
        bytes: encoded_patch.bytes,
    })
}

fn encode_release_patch(source: &[u8], target: &[u8]) -> Result<EncodedReleasePatch> {
    let mut patch = Vec::new();
    patch.extend_from_slice(BPS_MAGIC);
    encode_number(&mut patch, source.len() as u64);
    encode_number(&mut patch, target.len() as u64);
    encode_number(&mut patch, 0);
    let actions = release_patch_actions(source, target);
    let mut source_relative_offset = 0_i64;
    let mut action_byte_counts = PatchActionByteCounts::default();
    for action in actions {
        encode_action(
            &mut patch,
            action,
            &mut source_relative_offset,
            &mut action_byte_counts,
        )?;
    }
    patch.extend_from_slice(&crc32fast::hash(source).to_le_bytes());
    patch.extend_from_slice(&crc32fast::hash(target).to_le_bytes());
    let patch_crc32 = crc32fast::hash(&patch);
    patch.extend_from_slice(&patch_crc32.to_le_bytes());
    Ok(EncodedReleasePatch {
        bytes: patch,
        action_byte_counts,
    })
}

fn release_patch_actions<'a>(source: &[u8], target: &'a [u8]) -> Vec<PatchAction<'a>> {
    let source_match_index = SourceMatchIndex::new(source);
    let mut actions = Vec::new();
    let mut position = 0;
    while position < target.len() {
        if position < source.len() && source[position] == target[position] {
            let start = position;
            while position < target.len()
                && position < source.len()
                && source[position] == target[position]
            {
                position += 1;
            }
            actions.push(PatchAction::SourceRead(position - start));
        } else if let Some(source_match) =
            source_match_index.longest_match(source, target, position)
        {
            actions.push(PatchAction::SourceCopy {
                source_offset: source_match.source_offset,
                length: source_match.length,
            });
            position += source_match.length;
        } else {
            let start = position;
            position += 1;
            while position < target.len() {
                if position < source.len() && source[position] == target[position] {
                    break;
                }
                if source_match_index
                    .longest_match(source, target, position)
                    .is_some()
                {
                    break;
                }
                position += 1;
            }
            actions.push(PatchAction::TargetRead(&target[start..position]));
        }
    }
    actions
}

fn encode_action(
    patch: &mut Vec<u8>,
    action: PatchAction<'_>,
    source_relative_offset: &mut i64,
    byte_counts: &mut PatchActionByteCounts,
) -> Result<()> {
    match action {
        PatchAction::SourceRead(length) => {
            encode_number(patch, (length as u64 - 1) << 2);
            byte_counts.source_read += length;
        }
        PatchAction::SourceCopy {
            source_offset,
            length,
        } => {
            encode_number(patch, ((length as u64 - 1) << 2) | 2);
            let absolute_offset =
                i64::try_from(source_offset).context("BPS SourceCopy offset exceeds i64")?;
            let relative_delta = absolute_offset
                .checked_sub(*source_relative_offset)
                .context("BPS SourceCopy relative offset overflows")?;
            encode_signed_delta(patch, relative_delta)?;
            *source_relative_offset = absolute_offset
                .checked_add(i64::try_from(length).context("BPS SourceCopy length exceeds i64")?)
                .context("BPS SourceCopy cursor overflows")?;
            byte_counts.source_copy += length;
        }
        PatchAction::TargetRead(bytes) => {
            encode_number(patch, ((bytes.len() as u64 - 1) << 2) | 1);
            patch.extend_from_slice(bytes);
            byte_counts.target_read += bytes.len();
        }
    }
    Ok(())
}

fn encode_signed_delta(output: &mut Vec<u8>, delta: i64) -> Result<()> {
    let sign = u64::from(delta.is_negative());
    let encoded = delta
        .unsigned_abs()
        .checked_mul(2)
        .and_then(|magnitude| magnitude.checked_add(sign))
        .context("BPS relative delta does not fit its encoding")?;
    encode_number(output, encoded);
    Ok(())
}

fn encode_number(output: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            output.push(byte | 0x80);
            return;
        }
        output.push(byte);
        value -= 1;
    }
}

fn decode_number(input: &[u8], cursor: &mut usize, limit: usize) -> Result<u64> {
    let mut value = 0_u64;
    let mut shift = 1_u64;
    loop {
        ensure!(*cursor < limit, "unexpected end of BPS number");
        let byte = input[*cursor];
        *cursor += 1;
        value = value
            .checked_add(
                u64::from(byte & 0x7F)
                    .checked_mul(shift)
                    .context("BPS number overflows")?,
            )
            .context("BPS number overflows")?;
        if byte & 0x80 != 0 {
            return Ok(value);
        }
        shift = shift
            .checked_mul(128)
            .context("BPS number shift overflows")?;
        value = value.checked_add(shift).context("BPS number overflows")?;
    }
}

fn apply_release_patch(source: &[u8], patch: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        patch.len() >= BPS_MAGIC.len() + BPS_FOOTER_SIZE,
        "BPS patch is too small"
    );
    ensure!(
        &patch[..BPS_MAGIC.len()] == BPS_MAGIC,
        "BPS magic is not BPS1"
    );
    let footer = decode_footer(patch)?;
    ensure!(
        footer.patch_crc32 == crc32fast::hash(&patch[..patch.len() - 4]),
        "BPS patch CRC mismatch"
    );

    let footer_start = patch.len() - BPS_FOOTER_SIZE;
    let mut cursor = BPS_MAGIC.len();
    let source_size = decode_number(patch, &mut cursor, footer_start)?;
    let target_size = decode_number(patch, &mut cursor, footer_start)?;
    let metadata_size = usize::try_from(decode_number(patch, &mut cursor, footer_start)?)
        .context("BPS metadata length does not fit this host")?;
    cursor = cursor
        .checked_add(metadata_size)
        .context("BPS metadata range overflows")?;
    ensure!(
        cursor <= footer_start,
        "BPS metadata exceeds the patch body"
    );
    ensure!(
        source_size == source.len() as u64,
        "BPS source size mismatch: expected {source_size}, found {}",
        source.len()
    );
    ensure!(
        footer.source_crc32 == crc32fast::hash(source),
        "BPS source CRC mismatch"
    );

    let target_len = usize::try_from(target_size).context("BPS target length does not fit host")?;
    let mut target = vec![0; target_len];
    let mut output_offset = 0_usize;
    let mut source_relative_offset = 0_i64;
    let mut target_relative_offset = 0_i64;

    while cursor < footer_start {
        let encoded = decode_number(patch, &mut cursor, footer_start)?;
        let length =
            usize::try_from((encoded >> 2) + 1).context("BPS action length does not fit host")?;
        let output_end = output_offset
            .checked_add(length)
            .context("BPS output range overflows")?;
        ensure!(output_end <= target.len(), "BPS action exceeds target size");

        match encoded & 3 {
            0 => {
                ensure!(
                    output_end <= source.len(),
                    "BPS SourceRead exceeds source size"
                );
                target[output_offset..output_end]
                    .copy_from_slice(&source[output_offset..output_end]);
                output_offset = output_end;
            }
            1 => {
                let patch_end = cursor
                    .checked_add(length)
                    .context("BPS TargetRead range overflows")?;
                ensure!(
                    patch_end <= footer_start,
                    "BPS TargetRead exceeds patch body"
                );
                target[output_offset..output_end].copy_from_slice(&patch[cursor..patch_end]);
                cursor = patch_end;
                output_offset = output_end;
            }
            2 => {
                let delta = decode_signed_delta(patch, &mut cursor, footer_start)?;
                source_relative_offset = source_relative_offset
                    .checked_add(delta)
                    .context("BPS SourceCopy relative offset overflows")?;
                for output in &mut target[output_offset..output_end] {
                    let source_index = usize::try_from(source_relative_offset)
                        .context("BPS SourceCopy uses a negative source offset")?;
                    *output = *source
                        .get(source_index)
                        .context("BPS SourceCopy exceeds source size")?;
                    source_relative_offset = source_relative_offset
                        .checked_add(1)
                        .context("BPS SourceCopy cursor overflows")?;
                }
                output_offset = output_end;
            }
            3 => {
                let delta = decode_signed_delta(patch, &mut cursor, footer_start)?;
                target_relative_offset = target_relative_offset
                    .checked_add(delta)
                    .context("BPS TargetCopy relative offset overflows")?;
                while output_offset < output_end {
                    let target_index = usize::try_from(target_relative_offset)
                        .context("BPS TargetCopy uses a negative target offset")?;
                    ensure!(
                        target_index < output_offset,
                        "BPS TargetCopy reads data that has not been produced"
                    );
                    target[output_offset] = target[target_index];
                    output_offset += 1;
                    target_relative_offset = target_relative_offset
                        .checked_add(1)
                        .context("BPS TargetCopy cursor overflows")?;
                }
            }
            _ => unreachable!(),
        }
    }
    ensure!(
        output_offset == target.len(),
        "BPS output size mismatch: wrote {output_offset}, expected {}",
        target.len()
    );
    ensure!(
        footer.target_crc32 == crc32fast::hash(&target),
        "BPS target CRC mismatch"
    );
    Ok(target)
}

fn decode_signed_delta(input: &[u8], cursor: &mut usize, limit: usize) -> Result<i64> {
    let encoded = decode_number(input, cursor, limit)?;
    let magnitude = i64::try_from(encoded >> 1).context("BPS relative delta exceeds i64")?;
    Ok(if encoded & 1 == 0 {
        magnitude
    } else {
        -magnitude
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PatchFooter {
    source_crc32: u32,
    target_crc32: u32,
    patch_crc32: u32,
}

fn decode_footer(patch: &[u8]) -> Result<PatchFooter> {
    ensure!(patch.len() >= BPS_FOOTER_SIZE, "BPS patch has no footer");
    let start = patch.len() - BPS_FOOTER_SIZE;
    Ok(PatchFooter {
        source_crc32: u32::from_le_bytes(patch[start..start + 4].try_into()?),
        target_crc32: u32::from_le_bytes(patch[start + 4..start + 8].try_into()?),
        patch_crc32: u32::from_le_bytes(patch[start + 8..start + 12].try_into()?),
    })
}

fn write_release_patch_artifacts(
    source_path: &Path,
    release_path: &Path,
    output_path: &Path,
    report_path: &Path,
    patch_bytes: &[u8],
    report_bytes: &[u8],
) -> Result<()> {
    let output_identity = resolve_output_identity(output_path)?;
    let report_identity = resolve_output_identity(report_path)?;
    for protected_path in [source_path, release_path] {
        let protected_identity = fs::canonicalize(protected_path)
            .with_context(|| format!("resolve protected input {}", protected_path.display()))?;
        ensure!(
            output_identity != protected_identity && report_identity != protected_identity,
            "release patch artifacts must not overwrite protected input {}",
            protected_path.display()
        );
    }
    ensure!(
        output_identity != report_identity,
        "release patch output and report must use different paths"
    );

    write_and_verify(output_path, patch_bytes, "release patch")?;
    write_and_verify(report_path, report_bytes, "release patch report")
}

fn resolve_output_identity(path: &Path) -> Result<std::path::PathBuf> {
    if path.exists() {
        return fs::canonicalize(path)
            .with_context(|| format!("resolve existing path {}", path.display()));
    }
    let name = path
        .file_name()
        .context("release patch artifact path has no file name")?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("create output directory {}", parent.display()))?;
    Ok(fs::canonicalize(parent)
        .with_context(|| format!("resolve output directory {}", parent.display()))?
        .join(name))
}

fn write_and_verify(path: &Path, bytes: &[u8], role: &str) -> Result<()> {
    fs::write(path, bytes).with_context(|| format!("write {role} {}", path.display()))?;
    let read_back = fs::read(path).with_context(|| format!("read {role} {}", path.display()))?;
    ensure!(read_back == bytes, "{role} read-back differs from its plan");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_both_supported_source_layouts() {
        assert_eq!(
            release_patch_source_layout(rom::SOURCE_PAYLOAD_SIZE).unwrap(),
            ReleasePatchSourceLayout::Headerless
        );
        assert_eq!(
            release_patch_source_layout(rom::HEADER_SIZE + rom::SOURCE_PAYLOAD_SIZE).unwrap(),
            ReleasePatchSourceLayout::InesHeadered
        );
    }

    #[test]
    fn rejects_an_unsupported_source_layout() {
        let error = release_patch_source_layout(rom::SOURCE_PAYLOAD_SIZE + 1).unwrap_err();

        assert!(error.to_string().contains("headerless image"));
        assert!(error.to_string().contains("iNES image"));
    }

    #[test]
    fn resolves_a_new_artifact_in_the_current_directory() {
        let identity = resolve_output_identity(Path::new("plain-release.bps")).unwrap();

        assert_eq!(identity.file_name().unwrap(), "plain-release.bps");
        assert_eq!(identity.parent().unwrap(), fs::canonicalize(".").unwrap());
    }

    #[test]
    fn generated_patch_applies_to_byte_identical_target() {
        let mut source = vec![0xFF; 0x1_0000];
        for index in (0..source.len()).step_by(0x100) {
            source[index] = (index >> 8) as u8;
        }
        let mut target = source.clone();
        for index in 0..16 {
            target[index * 0x1000 + 0x42] = 0xAA;
        }

        let patch = create_verified_release_patch(&source, &target).unwrap();

        assert_eq!(&patch.bytes[..4], BPS_MAGIC);
        assert_eq!(apply_release_patch(&source, &patch.bytes).unwrap(), target);
        assert!(patch.report.apply_verified);
    }

    #[test]
    fn applying_patch_rejects_a_different_source() {
        let patch = encode_release_patch(b"source", b"target").unwrap();

        let error = apply_release_patch(b"wrong!", &patch.bytes).unwrap_err();

        assert!(error.to_string().contains("source CRC mismatch"));
    }

    #[test]
    fn generated_patch_applies_a_target_extension() {
        let source = b"supported-source";
        let target = b"supported-source-with-release-extension";

        let patch = create_verified_release_patch(source, target).unwrap();

        assert_eq!(apply_release_patch(source, &patch.bytes).unwrap(), target);
        assert_eq!(patch.report.source_size, source.len());
        assert_eq!(patch.report.target_size, target.len());
    }

    #[test]
    fn relocated_source_block_uses_source_copy_instead_of_literal_bytes() {
        let mut source = vec![0x11; 8 * 1024];
        source.extend_from_slice(&vec![0xA5; 8 * 1024]);
        let target = vec![0xA5; 8 * 1024];

        let patch = create_verified_release_patch(&source, &target).unwrap();

        assert_eq!(apply_release_patch(&source, &patch.bytes).unwrap(), target);
        assert_eq!(patch.report.source_read_byte_count, 0);
        assert_eq!(patch.report.source_copy_byte_count, target.len());
        assert_eq!(patch.report.target_read_byte_count, 0);
        assert!(patch.bytes.len() < 64);
    }

    #[test]
    fn crc32_matches_the_standard_check_vector() {
        assert_eq!(crc32fast::hash(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn encoded_numbers_roundtrip_at_boundaries() {
        for value in [0, 1, 127, 128, 255, 256, 16_383, 16_384, u32::MAX as u64] {
            let mut encoded = Vec::new();
            encode_number(&mut encoded, value);
            let mut cursor = 0;
            assert_eq!(
                decode_number(&encoded, &mut cursor, encoded.len()).unwrap(),
                value
            );
            assert_eq!(cursor, encoded.len());
        }
    }
}
