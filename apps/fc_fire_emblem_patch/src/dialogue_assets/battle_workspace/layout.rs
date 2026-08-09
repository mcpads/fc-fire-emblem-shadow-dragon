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
    rom.verify_supported_japanese()?;
    let workspace_bytes = fs::read(workspace_path)
        .with_context(|| format!("read {}", workspace_path.display()))?;
    let workspace: BattleDialogueWorkspace = serde_json::from_slice(&workspace_bytes)
        .with_context(|| format!("parse {}", workspace_path.display()))?;
    validate_workspace_binding(rom.data(), &workspace)?;
    validate_translation_fields(&workspace)?;
    let source_records = inspect_battle_dialogue_translation_records(rom.data())?;
    let physical = inspect_battle_dialogue_physical_layout(rom.data())?;
    ensure!(
        source_records.len() == workspace.records.len(),
        "battle layout lost workspace records"
    );

    let record_sizes = workspace
        .records
        .iter()
        .map(|record| {
            record.lines.iter().try_fold(4usize, |total, line| {
                let line_size = if line.status == TranslationStatus::Untranslated {
                    source_markup_byte_count(&line.source_markup)?
                } else {
                    encode_korean_markup(&line.korean)?.len()
                };
                total.checked_add(line_size).context("battle record size overflow")
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let translated_record_storage_byte_count = record_sizes.iter().sum::<usize>();
    let preserved_storage_byte_count = physical.preserved_unreferenced_end_file_offset_exclusive
        - physical.preserved_unreferenced_file_offset;
    let segments = [
        (
            physical.data_file_start,
            physical.preserved_unreferenced_file_offset,
        ),
        (
            physical.preserved_unreferenced_end_file_offset_exclusive,
            physical.data_file_end_exclusive,
        ),
    ];
    let placements = pack_record_sizes(&record_sizes, &segments)?;
    let mut records = Vec::new();
    for ((source_record, planned_size), cursor) in source_records
        .iter()
        .zip(record_sizes)
        .zip(placements)
    {
        let planned_pointer = switchable_file_to_cpu(source_record.source_prg_bank, cursor)?;
        records.push(BattleRecordLayoutReport {
            canonical_entry_index: source_record.canonical_entry_index,
            entry_indices: source_record.entry_indices.clone(),
            pointer_file_offsets_hex: source_record
                .pointer_file_offsets
                .iter()
                .map(|offset| format!("0x{offset:05X}"))
                .collect(),
            planned_pointer_cpu_address_hex: format!("0x{planned_pointer:04X}"),
            planned_file_offset_hex: format!("0x{cursor:05X}"),
            planned_storage_byte_count: planned_size,
        });
    }
    let capacity_byte_count = physical.data_file_end_exclusive - physical.data_file_start;
    let remaining_storage_byte_count = capacity_byte_count
        - translated_record_storage_byte_count
        - preserved_storage_byte_count;
    let translation_input_complete = workspace
        .records
        .iter()
        .flat_map(|record| &record.lines)
        .filter(|line| line.japanese_source_byte_count > 0)
        .all(|line| line.status != TranslationStatus::Untranslated);
    let pointer_write_count = records
        .iter()
        .map(|record| record.pointer_file_offsets_hex.len())
        .sum();
    let report = BattleDialogueLayoutReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        workspace_sha1: sha1_hex(&workspace_bytes),
        dialogue_content_emitted: false,
        glyph_characters_emitted: false,
        capacity_byte_count,
        translated_record_storage_byte_count,
        preserved_unreferenced_storage_byte_count: preserved_storage_byte_count,
        remaining_storage_byte_count,
        preserved_unreferenced_storage_sha1: physical.preserved_unreferenced_storage_sha1,
        records,
        translation_input_complete,
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
        translated_record_storage_byte_count,
        preserved_storage_byte_count,
        remaining_storage_byte_count,
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
