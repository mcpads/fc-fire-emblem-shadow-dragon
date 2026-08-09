use super::*;

#[derive(Debug)]
pub(crate) struct BattleDialogueLayoutSummary {
    pub(crate) report_sha1: String,
    pub(crate) record_count: usize,
    pub(crate) pointer_write_count: usize,
    pub(crate) translated_record_storage_byte_count: usize,
    pub(crate) preserved_storage_byte_count: usize,
    pub(crate) remaining_storage_byte_count: usize,
}

#[derive(Debug, Serialize)]
struct BattleDialogueLayoutReport {
    schema: u8,
    source_sha1: &'static str,
    workspace_sha1: String,
    dialogue_content_emitted: bool,
    glyph_characters_emitted: bool,
    capacity_byte_count: usize,
    translated_record_storage_byte_count: usize,
    preserved_unreferenced_storage_byte_count: usize,
    remaining_storage_byte_count: usize,
    preserved_unreferenced_storage_sha1: String,
    records: Vec<BattleRecordLayoutReport>,
    translation_input_complete: bool,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct BattleRecordLayoutReport {
    canonical_entry_index: usize,
    entry_indices: Vec<usize>,
    pointer_file_offsets_hex: Vec<String>,
    planned_pointer_cpu_address_hex: String,
    planned_file_offset_hex: String,
    planned_storage_byte_count: usize,
}

pub(crate) fn plan_battle_dialogue_reinsertion(
    source_path: &Path,
    workspace_path: &Path,
    report_path: &Path,
) -> Result<BattleDialogueLayoutSummary> {
    let rom = Rom::from_path(source_path)?;
    let plan = plan_battle_dialogue_records(&rom, workspace_path)?;
    let records = plan
        .records
        .iter()
        .map(|record| BattleRecordLayoutReport {
            canonical_entry_index: record.canonical_entry_index,
            entry_indices: record.entry_indices.clone(),
            pointer_file_offsets_hex: record
                .pointer_file_offsets
                .iter()
                .map(|offset| format!("0x{offset:05X}"))
                .collect(),
            planned_pointer_cpu_address_hex: format!(
                "0x{:04X}",
                record.planned_pointer_cpu_address
            ),
            planned_file_offset_hex: format!("0x{:05X}", record.planned_file_offset),
            planned_storage_byte_count: record.storage_byte_count(),
        })
        .collect::<Vec<_>>();
    let pointer_write_count = records
        .iter()
        .map(|record| record.pointer_file_offsets_hex.len())
        .sum();
    let preserved_storage_byte_count = plan.preserved_unreferenced_end_file_offset_exclusive
        - plan.preserved_unreferenced_file_offset;
    let report = BattleDialogueLayoutReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        workspace_sha1: plan.workspace_sha1,
        dialogue_content_emitted: false,
        glyph_characters_emitted: false,
        capacity_byte_count: plan.capacity_byte_count,
        translated_record_storage_byte_count: plan.translated_record_storage_byte_count,
        preserved_unreferenced_storage_byte_count: preserved_storage_byte_count,
        remaining_storage_byte_count: plan.remaining_storage_byte_count,
        preserved_unreferenced_storage_sha1: plan.preserved_unreferenced_storage_sha1,
        records,
        translation_input_complete: true,
        release_eligible: false,
    };
    let mut report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize battle layout report")?;
    report_bytes.push(b'\n');
    write_file(report_path, &report_bytes)?;
    Ok(BattleDialogueLayoutSummary {
        report_sha1: sha1_hex(&report_bytes),
        record_count: report.records.len(),
        pointer_write_count,
        translated_record_storage_byte_count: plan.translated_record_storage_byte_count,
        preserved_storage_byte_count,
        remaining_storage_byte_count: plan.remaining_storage_byte_count,
    })
}

pub(super) fn pack_record_sizes(
    record_sizes: &[usize],
    segments: &[(usize, usize)],
) -> Result<Vec<usize>> {
    ensure!(!segments.is_empty(), "battle layout has no owned segments");
    for &(start, end_exclusive) in segments {
        ensure!(start <= end_exclusive, "battle layout segment is inverted");
    }

    let mut segment_index = 0;
    let mut cursor = segments[0].0;
    let mut placements = Vec::with_capacity(record_sizes.len());
    for &record_size in record_sizes {
        let end = cursor
            .checked_add(record_size)
            .context("battle record placement overflow")?;
        if end > segments[segment_index].1 {
            segment_index += 1;
            ensure!(
                segment_index < segments.len(),
                "battle records exceed owned segments"
            );
            cursor = segments[segment_index].0;
        }
        let end = cursor
            .checked_add(record_size)
            .context("battle record placement overflow")?;
        ensure!(
            end <= segments[segment_index].1,
            "battle record crosses preserved storage"
        );
        placements.push(cursor);
        cursor = end;
    }
    Ok(placements)
}
