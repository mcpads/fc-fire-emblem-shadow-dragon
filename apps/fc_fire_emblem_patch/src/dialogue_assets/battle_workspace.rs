use std::collections::BTreeMap;

use super::*;

mod build_plan;
mod layout;
#[cfg(test)]
mod layout_tests;

pub(crate) use build_plan::{
    BattleDialogueReinsertionPlan, EncodedBattleDialogueRecord, plan_battle_dialogue_records,
};
pub(crate) use layout::plan_battle_dialogue_reinsertion;

#[derive(Debug)]
pub(crate) struct BattleDialogueWorkspaceSummary {
    pub(crate) workspace_sha1: String,
    pub(crate) record_count: usize,
    pub(crate) line_count: usize,
    pub(crate) japanese_source_byte_count: usize,
    pub(crate) preserved_translation_line_count: usize,
}

#[derive(Debug)]
pub(crate) struct BattleDialogueValidationSummary {
    pub(crate) workspace_sha1: String,
    pub(crate) record_count: usize,
    pub(crate) line_count: usize,
    pub(crate) filled_line_count: usize,
    pub(crate) complete_line_count: usize,
    pub(crate) untranslated_japanese_line_count: usize,
    pub(crate) target_glyph_count: usize,
    pub(crate) translated_record_storage_byte_count: usize,
    pub(crate) preserved_unreferenced_storage_byte_count: usize,
    pub(crate) planned_storage_byte_count: usize,
    pub(crate) remaining_storage_byte_count: usize,
    pub(crate) translation_input_complete: bool,
    pub(crate) review_complete: bool,
}

#[derive(Debug)]
pub(crate) struct BattleDialogueDraftImportSummary {
    pub(crate) workspace_sha1: String,
    pub(crate) imported_line_count: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct BattleDialogueWorkspace {
    format_version: u8,
    source_sha1: String,
    translate_from: String,
    translate_to: String,
    preserve_existing_english: bool,
    purpose: String,
    records: Vec<BattleDialogueWorkspaceRecord>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct BattleDialogueWorkspaceRecord {
    id: String,
    table_id: String,
    source_prg_bank: u8,
    canonical_entry_index: usize,
    entry_indices: Vec<usize>,
    pointer_cpu_address_hex: String,
    pointer_file_offsets_hex: Vec<String>,
    file_offset_hex: String,
    end_file_offset_exclusive_hex: String,
    source_storage_sha1: String,
    header_hex: String,
    lines: Vec<BattleDialogueWorkspaceLine>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
struct BattleDialogueWorkspaceLine {
    id: String,
    index: usize,
    source_markup: String,
    korean: String,
    status: TranslationStatus,
    japanese_source_byte_count: usize,
}

pub(crate) fn extract_battle_dialogue_workspace(
    source_path: &Path,
    workspace_path: &Path,
) -> Result<BattleDialogueWorkspaceSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let records = inspect_battle_dialogue_translation_records(rom.data())?;
    let mut japanese_source_byte_count = 0;
    let workspace_records = records
        .into_iter()
        .map(|record| {
            ensure!(
                record.pointer_file_offsets.len() == record.entry_indices.len(),
                "battle-dialogue alias pointer coverage changed"
            );
            for pointer_file_offset in &record.pointer_file_offsets {
                ensure!(
                    rom.data()
                        .get(*pointer_file_offset..*pointer_file_offset + 2)
                        == Some(&record.pointer_cpu_address.to_le_bytes()),
                    "battle-dialogue pointer source bytes changed"
                );
            }
            let lines = decode_battle_record_lines(rom.data(), &record)?;
            japanese_source_byte_count += lines
                .iter()
                .map(|line| line.japanese_source_byte_count)
                .sum::<usize>();
            Ok(BattleDialogueWorkspaceRecord {
                id: format!("{}:{:03}", record.table_id, record.canonical_entry_index),
                table_id: record.table_id.to_owned(),
                source_prg_bank: record.source_prg_bank,
                canonical_entry_index: record.canonical_entry_index,
                entry_indices: record.entry_indices,
                pointer_cpu_address_hex: format!("0x{:04X}", record.pointer_cpu_address),
                pointer_file_offsets_hex: record
                    .pointer_file_offsets
                    .iter()
                    .map(|offset| format!("0x{offset:05X}"))
                    .collect(),
                file_offset_hex: format!("0x{:05X}", record.file_offset),
                end_file_offset_exclusive_hex: format!(
                    "0x{:05X}",
                    record.end_file_offset_exclusive
                ),
                source_storage_sha1: record.storage_sha1,
                header_hex: record.header_hex,
                lines,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let line_count = workspace_records
        .iter()
        .map(|record| record.lines.len())
        .sum();
    let mut workspace = BattleDialogueWorkspace {
        format_version: 1,
        source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
        translate_from: "ja".to_owned(),
        translate_to: "ko".to_owned(),
        preserve_existing_english: true,
        purpose: "private_battle_dialogue_translation_workspace".to_owned(),
        records: workspace_records,
    };
    let preserved_translation_line_count = if workspace_path.exists() {
        let existing_bytes = fs::read(workspace_path)
            .with_context(|| format!("read {}", workspace_path.display()))?;
        let existing: BattleDialogueWorkspace = serde_json::from_slice(&existing_bytes)
            .with_context(|| format!("parse {}", workspace_path.display()))?;
        preserve_translations(&mut workspace, &existing)?
    } else {
        0
    };
    let mut bytes = serde_json::to_vec_pretty(&workspace)
        .context("serialize battle-dialogue translation workspace")?;
    bytes.push(b'\n');
    write_file_atomically(workspace_path, &bytes)?;
    Ok(BattleDialogueWorkspaceSummary {
        workspace_sha1: sha1_hex(&bytes),
        record_count: workspace.records.len(),
        line_count,
        japanese_source_byte_count,
        preserved_translation_line_count,
    })
}

pub(crate) fn validate_battle_dialogue_workspace(
    source_path: &Path,
    workspace_path: &Path,
) -> Result<BattleDialogueValidationSummary> {
    let rom = Rom::from_path(source_path)?;
    rom.verify_supported_japanese()?;
    let workspace_bytes =
        fs::read(workspace_path).with_context(|| format!("read {}", workspace_path.display()))?;
    let workspace: BattleDialogueWorkspace = serde_json::from_slice(&workspace_bytes)
        .with_context(|| format!("parse {}", workspace_path.display()))?;
    validate_workspace_binding(rom.data(), &workspace)?;
    let (
        filled_line_count,
        complete_line_count,
        untranslated_japanese_line_count,
        target_glyph_count,
    ) = validate_translation_fields(&workspace)?;
    let translated_record_storage_byte_count = planned_storage_byte_count(&workspace)?;
    let preserved_unreferenced_storage_byte_count = 16;
    let planned_storage_byte_count =
        translated_record_storage_byte_count + preserved_unreferenced_storage_byte_count;
    let physical_storage_byte_count = 1_168;
    ensure!(
        planned_storage_byte_count <= physical_storage_byte_count,
        "battle translations require {planned_storage_byte_count} bytes but the owned physical region has {physical_storage_byte_count}"
    );
    Ok(BattleDialogueValidationSummary {
        workspace_sha1: sha1_hex(&workspace_bytes),
        record_count: workspace.records.len(),
        line_count: workspace
            .records
            .iter()
            .map(|record| record.lines.len())
            .sum(),
        filled_line_count,
        complete_line_count,
        untranslated_japanese_line_count,
        target_glyph_count,
        translated_record_storage_byte_count,
        preserved_unreferenced_storage_byte_count,
        planned_storage_byte_count,
        remaining_storage_byte_count: physical_storage_byte_count - planned_storage_byte_count,
        translation_input_complete: untranslated_japanese_line_count == 0,
        review_complete: untranslated_japanese_line_count == 0
            && complete_line_count == filled_line_count,
    })
}

pub(crate) fn import_battle_dialogue_draft(
    workspace_path: &Path,
    draft_path: &Path,
) -> Result<BattleDialogueDraftImportSummary> {
    let workspace_bytes =
        fs::read(workspace_path).with_context(|| format!("read {}", workspace_path.display()))?;
    let mut workspace: BattleDialogueWorkspace = serde_json::from_slice(&workspace_bytes)
        .with_context(|| format!("parse {}", workspace_path.display()))?;
    let draft =
        fs::read_to_string(draft_path).with_context(|| format!("read {}", draft_path.display()))?;
    let mut translations = BTreeMap::new();
    for (line_number, row) in draft.lines().enumerate() {
        if row.is_empty() || row.starts_with('#') {
            continue;
        }
        let (id, korean) = row
            .split_once('\t')
            .with_context(|| format!("draft line {} has no tab separator", line_number + 1))?;
        ensure!(!korean.is_empty(), "draft translation {id} is empty");
        ensure!(
            translations
                .insert(id.to_owned(), korean.to_owned())
                .is_none(),
            "draft contains duplicate line {id}"
        );
    }
    let expected_ids = workspace
        .records
        .iter()
        .flat_map(|record| &record.lines)
        .filter(|line| line.japanese_source_byte_count > 0)
        .map(|line| line.id.clone())
        .collect::<BTreeSet<_>>();
    let actual_ids = translations.keys().cloned().collect::<BTreeSet<_>>();
    ensure!(
        actual_ids == expected_ids,
        "battle draft must cover every Japanese-bearing line exactly once"
    );
    for line in workspace
        .records
        .iter_mut()
        .flat_map(|record| &mut record.lines)
    {
        if let Some(korean) = translations.get(&line.id) {
            line.korean.clone_from(korean);
            line.status = TranslationStatus::NeedsHumanReview;
        }
    }
    validate_translation_fields(&workspace)?;
    let mut output = serde_json::to_vec_pretty(&workspace)
        .context("serialize imported battle-dialogue workspace")?;
    output.push(b'\n');
    write_file_atomically(workspace_path, &output)?;
    Ok(BattleDialogueDraftImportSummary {
        workspace_sha1: sha1_hex(&output),
        imported_line_count: translations.len(),
    })
}

fn preserve_translations(
    fresh: &mut BattleDialogueWorkspace,
    existing: &BattleDialogueWorkspace,
) -> Result<usize> {
    ensure!(
        existing.format_version == 1,
        "battle workspace format changed"
    );
    let mut existing_header = existing.clone();
    existing_header.records.clear();
    let mut fresh_header = fresh.clone();
    fresh_header.records.clear();
    ensure!(
        existing_header == fresh_header,
        "battle workspace translation scope changed"
    );
    validate_translation_fields(existing)?;
    ensure!(
        existing.records.len() == fresh.records.len(),
        "battle workspace record count changed"
    );

    let mut preserved = 0;
    for (fresh_record, existing_record) in fresh.records.iter_mut().zip(&existing.records) {
        let mut fresh_binding = fresh_record.clone();
        fresh_binding.lines.clear();
        let mut existing_binding = existing_record.clone();
        existing_binding.lines.clear();
        ensure!(
            fresh_binding == existing_binding,
            "battle workspace record binding changed at {}",
            fresh_record.id
        );
        ensure!(
            fresh_record.lines.len() == existing_record.lines.len(),
            "battle workspace line count changed at {}",
            fresh_record.id
        );
        for (fresh_line, existing_line) in fresh_record.lines.iter_mut().zip(&existing_record.lines)
        {
            let mut fresh_line_binding = fresh_line.clone();
            fresh_line_binding.korean.clear();
            fresh_line_binding.status = TranslationStatus::Untranslated;
            let mut existing_line_binding = existing_line.clone();
            existing_line_binding.korean.clear();
            existing_line_binding.status = TranslationStatus::Untranslated;
            if source_markup_matches_current_punctuation_decode(
                &existing_line_binding.source_markup,
                &fresh_line_binding.source_markup,
            ) {
                existing_line_binding
                    .source_markup
                    .clone_from(&fresh_line_binding.source_markup);
            }
            ensure!(
                fresh_line_binding == existing_line_binding,
                "battle workspace source changed at {}",
                fresh_line.id
            );
            if existing_line.status != TranslationStatus::Untranslated {
                ensure!(
                    !existing_line.korean.is_empty(),
                    "translated battle line {} is empty",
                    existing_line.id
                );
                fresh_line.korean.clone_from(&existing_line.korean);
                fresh_line.status = existing_line.status;
                preserved += 1;
            }
        }
    }
    Ok(preserved)
}

fn validate_workspace_binding(source: &[u8], workspace: &BattleDialogueWorkspace) -> Result<()> {
    ensure!(
        workspace.format_version == 1
            && workspace.source_sha1 == EXPECTED_SOURCE_SHA1
            && workspace.translate_from == "ja"
            && workspace.translate_to == "ko"
            && workspace.preserve_existing_english
            && workspace.purpose == "private_battle_dialogue_translation_workspace",
        "battle workspace header changed"
    );
    let records = inspect_battle_dialogue_translation_records(source)?;
    ensure!(
        workspace.records.len() == records.len(),
        "battle workspace record count changed"
    );
    for (actual, source_record) in workspace.records.iter().zip(&records) {
        ensure!(
            actual.id
                == format!(
                    "{}:{:03}",
                    source_record.table_id, source_record.canonical_entry_index
                )
                && actual.table_id == source_record.table_id
                && actual.source_prg_bank == source_record.source_prg_bank
                && actual.canonical_entry_index == source_record.canonical_entry_index
                && actual.entry_indices == source_record.entry_indices
                && actual.pointer_cpu_address_hex
                    == format!("0x{:04X}", source_record.pointer_cpu_address)
                && actual.pointer_file_offsets_hex
                    == source_record
                        .pointer_file_offsets
                        .iter()
                        .map(|offset| format!("0x{offset:05X}"))
                        .collect::<Vec<_>>()
                && actual.file_offset_hex == format!("0x{:05X}", source_record.file_offset)
                && actual.end_file_offset_exclusive_hex
                    == format!("0x{:05X}", source_record.end_file_offset_exclusive)
                && actual.source_storage_sha1 == source_record.storage_sha1
                && actual.header_hex == source_record.header_hex,
            "battle workspace record binding changed at {}",
            actual.id
        );
        let expected_lines = decode_battle_record_lines(source, source_record)?;
        ensure!(
            actual.lines.len() == expected_lines.len(),
            "battle workspace line count changed at {}",
            actual.id
        );
        for (actual_line, expected_line) in actual.lines.iter().zip(expected_lines) {
            let mut binding = actual_line.clone();
            binding.korean.clear();
            binding.status = TranslationStatus::Untranslated;
            if source_markup_matches_current_punctuation_decode(
                &binding.source_markup,
                &expected_line.source_markup,
            ) {
                binding
                    .source_markup
                    .clone_from(&expected_line.source_markup);
            }
            ensure!(
                binding == expected_line,
                "battle workspace source binding changed at {}",
                actual_line.id
            );
        }
    }
    Ok(())
}

fn source_markup_matches_current_punctuation_decode(existing: &str, current: &str) -> bool {
    existing == current || existing.replace('。', "、").replace("{LIT:8F}", "。") == current
}

fn validate_translation_fields(
    workspace: &BattleDialogueWorkspace,
) -> Result<(usize, usize, usize, usize)> {
    let mut filled = 0;
    let mut complete = 0;
    let mut untranslated_japanese = 0;
    let mut target_glyphs = 0;
    for line in workspace.records.iter().flat_map(|record| &record.lines) {
        if line.status == TranslationStatus::Untranslated {
            ensure!(
                line.korean.is_empty(),
                "{} is untranslated but not empty",
                line.id
            );
            untranslated_japanese += usize::from(line.japanese_source_byte_count != 0);
            continue;
        }
        ensure!(
            !line.korean.is_empty(),
            "{} is translated but empty",
            line.id
        );
        let source = inspect_markup(&line.source_markup, MarkupRole::Source)
            .with_context(|| format!("inspect battle source at {}", line.id))?;
        let target = inspect_markup(&line.korean, MarkupRole::KoreanTarget)
            .with_context(|| format!("inspect battle target at {}", line.id))?;
        ensure!(
            source.protected_items == target.protected_items,
            "{} changed a control token or existing English/digit literal",
            line.id
        );
        validate_battle_variable_postpositions(line)?;
        let final_control = source
            .protected_items
            .last()
            .filter(|item| item.starts_with('{'))
            .context("battle source line has no final control")?;
        ensure!(
            line.korean.ends_with(final_control),
            "{} moved its final control token",
            line.id
        );
        filled += 1;
        complete += usize::from(line.status == TranslationStatus::Complete);
        target_glyphs += target.editable_glyph_count;
    }
    Ok((filled, complete, untranslated_japanese, target_glyphs))
}

fn validate_battle_variable_postpositions(line: &BattleDialogueWorkspaceLine) -> Result<()> {
    validate_neutral_variable_postpositions(
        &line.id,
        &line.korean,
        |dynamic_value, bare_postposition| {
            line.id == "battle-dialogue:001:line:01"
                && dynamic_value == "{EC:01}"
                && bare_postposition == "로"
        },
    )
}

fn planned_storage_byte_count(workspace: &BattleDialogueWorkspace) -> Result<usize> {
    workspace.records.iter().try_fold(0usize, |total, record| {
        let body_byte_count = record.lines.iter().try_fold(0usize, |record_total, line| {
            let line_byte_count = if line.status == TranslationStatus::Untranslated {
                source_markup_byte_count(&line.source_markup)?
            } else {
                encode_korean_markup(&line.korean)?.len()
            };
            record_total
                .checked_add(line_byte_count)
                .context("battle record planned size overflow")
        })?;
        total
            .checked_add(4 + body_byte_count)
            .context("battle workspace planned size overflow")
    })
}

fn source_markup_byte_count(markup: &str) -> Result<usize> {
    let mut byte_count = 0usize;
    let mut chars = markup.char_indices().peekable();
    while let Some((start, character)) = chars.next() {
        if character == '{' {
            let end = chars
                .by_ref()
                .find_map(|(index, candidate)| (candidate == '}').then_some(index))
                .context("battle source markup token has no closing brace")?;
            byte_count += decode_protected_token(&markup[start..=end])?.len();
            continue;
        }
        ensure!(character != '}', "battle source markup has unmatched brace");
        ensure!(
            encode_protected_literal(character).is_some()
                || (0..=u8::MAX).any(|code| {
                    japanese_text_glyph(code).is_some_and(|glyph| glyph == character.to_string())
                }),
            "battle source markup contains an unencodable character {character:?}"
        );
        byte_count += 1;
    }
    Ok(byte_count)
}

fn decode_battle_record_lines(
    source: &[u8],
    record: &crate::dialogue_inventory::BattleDialogueTranslationRecord,
) -> Result<Vec<BattleDialogueWorkspaceLine>> {
    let literal_offsets = record
        .literal_file_offsets
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let mut lines = Vec::new();
    let mut markup = String::new();
    let mut japanese_count = 0;
    let mut cursor = record.file_offset + 4;
    while cursor < record.end_file_offset_exclusive {
        let code = source[cursor];
        if literal_offsets.contains(&cursor) {
            append_literal_markup(&mut markup, code);
            japanese_count += usize::from(is_japanese_text_code(code));
            cursor += 1;
            continue;
        }
        let operand_count = usize::from(code == 0xEC);
        let end = cursor + 1 + operand_count;
        ensure!(
            end <= record.end_file_offset_exclusive,
            "battle-dialogue control crosses its record"
        );
        markup.push('{');
        markup.push_str(&format!("{code:02X}"));
        if operand_count == 1 {
            markup.push(':');
            markup.push_str(&format!("{:02X}", source[cursor + 1]));
        }
        markup.push('}');
        cursor = end;
        if matches!(code, 0xED..=0xEF) {
            let index = lines.len();
            lines.push(BattleDialogueWorkspaceLine {
                id: format!(
                    "{}:{:03}:line:{index:02}",
                    record.table_id, record.canonical_entry_index
                ),
                index,
                source_markup: std::mem::take(&mut markup),
                korean: String::new(),
                status: TranslationStatus::Untranslated,
                japanese_source_byte_count: japanese_count,
            });
            japanese_count = 0;
        }
    }
    ensure!(
        markup.is_empty(),
        "battle-dialogue record has an unterminated line"
    );
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_previous_punctuation_decode_for_translation_preservation() {
        assert!(source_markup_matches_current_punctuation_decode(
            "{AB}あ。{AC}{ED}",
            "{AB}あ、{AC}{ED}"
        ));
        assert!(!source_markup_matches_current_punctuation_decode(
            "{AB}い。{AC}{ED}",
            "{AB}あ、{AC}{ED}"
        ));
    }

    #[test]
    fn battle_workspace_lines_keep_dynamic_values_controls_and_existing_english_protected() {
        let source = [
            0x08, 0x13, 0x10, 0x04, 0xEC, 0x02, 0x00, 0x6A, 0xAB, 0xED, 0x01, 0xAC, 0xEF,
        ];
        let record = crate::dialogue_inventory::BattleDialogueTranslationRecord {
            table_id: "battle-dialogue",
            source_prg_bank: 4,
            canonical_entry_index: 0,
            entry_indices: vec![0],
            pointer_cpu_address: 0x8000,
            pointer_file_offsets: vec![0],
            file_offset: 0,
            end_file_offset_exclusive: source.len(),
            storage_sha1: "storage".to_owned(),
            header_hex: "08131004".to_owned(),
            literal_file_offsets: vec![6, 7, 10],
        };

        let lines = decode_battle_record_lines(&source, &record).unwrap();

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].source_markup, "{EC:02}あA{AB}{ED}");
        assert_eq!(lines[0].japanese_source_byte_count, 1);
        assert_eq!(lines[1].source_markup, "い{AC}{EF}");
        assert_eq!(lines[1].japanese_source_byte_count, 1);
    }

    #[test]
    fn battle_translation_validation_rejects_changed_dynamic_tokens() {
        let workspace = BattleDialogueWorkspace {
            format_version: 1,
            source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
            translate_from: "ja".to_owned(),
            translate_to: "ko".to_owned(),
            preserve_existing_english: true,
            purpose: "private_battle_dialogue_translation_workspace".to_owned(),
            records: vec![BattleDialogueWorkspaceRecord {
                id: "battle-dialogue:000".to_owned(),
                table_id: "battle-dialogue".to_owned(),
                source_prg_bank: 4,
                canonical_entry_index: 0,
                entry_indices: vec![0],
                pointer_cpu_address_hex: "0x8000".to_owned(),
                pointer_file_offsets_hex: vec!["0x00000".to_owned()],
                file_offset_hex: "0x00000".to_owned(),
                end_file_offset_exclusive_hex: "0x00001".to_owned(),
                source_storage_sha1: "storage".to_owned(),
                header_hex: "08131004".to_owned(),
                lines: vec![BattleDialogueWorkspaceLine {
                    id: "battle-dialogue:000:line:00".to_owned(),
                    index: 0,
                    source_markup: "{EC:02}はA{ED}".to_owned(),
                    korean: "{EC:01}은A{ED}".to_owned(),
                    status: TranslationStatus::NeedsHumanReview,
                    japanese_source_byte_count: 1,
                }],
            }],
        };

        let error = validate_translation_fields(&workspace).unwrap_err();
        assert!(error.to_string().contains("changed a control token"));
    }

    #[test]
    fn battle_variable_names_reject_a_bare_korean_postposition() {
        let line = BattleDialogueWorkspaceLine {
            id: "battle-dialogue:005:line:00".to_owned(),
            index: 0,
            source_markup: "{EC:02}は{SP}たおれた{EE}".to_owned(),
            korean: "{EC:02}는{SP}쓰러졌다{EE}".to_owned(),
            status: TranslationStatus::NeedsHumanReview,
            japanese_source_byte_count: 4,
        };

        let error = validate_battle_variable_postpositions(&line)
            .unwrap_err()
            .to_string();
        assert!(error.contains("attaches bare postposition"));

        let neutral = BattleDialogueWorkspaceLine {
            korean: "{EC:02}은(는){SP}쓰러졌다{EE}".to_owned(),
            ..line
        };
        assert!(validate_battle_variable_postpositions(&neutral).is_ok());
    }

    #[test]
    fn battle_markup_size_counts_dynamic_operands_and_quote_controls() {
        assert_eq!(source_markup_byte_count("{EC:02}あA{AB}{ED}").unwrap(), 6);
        assert_eq!(encode_korean_markup("{AB}한{AC}{ED}").unwrap().len(), 4);
    }
}
