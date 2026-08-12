use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use super::*;

pub(super) fn build_entry_mode_workspace_without_seed(source: &[u8]) -> Result<EntryModeWorkspace> {
    let inspection = crate::dialogue_inventory::inspect_main_dialogue_entry_modes(source)?;
    let differing_entry_start_japanese_source_byte_count = inspection
        .transition_targets
        .iter()
        .map(differing_entry_start_japanese_byte_count)
        .sum::<Result<usize>>()?;
    let records = inspection
        .transition_targets
        .iter()
        .map(|target| build_entry_mode_record(source, target))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        records.len() == 139,
        "main-dialogue entry-mode workspace must contain all 139 dual-entry records"
    );
    let workspace = EntryModeWorkspace {
        format_version: ENTRY_MODE_WORKSPACE_FORMAT_VERSION,
        source_sha1: EXPECTED_SOURCE_SHA1.to_owned(),
        translate_from: "ja".to_owned(),
        translate_to: "ko".to_owned(),
        preserve_existing_english: true,
        purpose: WORKSPACE_PURPOSE.to_owned(),
        reachability_policy: REACHABILITY_POLICY.to_owned(),
        required_entry_modes: REQUIRED_ENTRY_MODES.map(str::to_owned),
        differing_entry_start_japanese_source_byte_count,
        records,
    };
    ensure!(
        workspace.differing_entry_start_japanese_source_byte_count == 288,
        "main-dialogue differing entry-start Japanese population changed"
    );
    Ok(workspace)
}

fn build_entry_mode_record(
    source: &[u8],
    target: &crate::dialogue_inventory::MainDialogueTransitionTargetMode,
) -> Result<EntryModeRecord> {
    let direct_start = target
        .record_file_offset
        .checked_add(target.direct_prefix_byte_count)
        .context("direct entry-mode start overflow")?;
    let transition_start = target
        .record_file_offset
        .checked_add(target.transition_prefix_byte_count)
        .context("transition entry-mode start overflow")?;
    ensure!(
        target.direct_lines.first().map(|line| line.file_offset) == Some(direct_start),
        "{} direct prefix no longer reaches its first line",
        target.record_id
    );
    ensure!(
        target.transition_lines.first().map(|line| line.file_offset) == Some(transition_start),
        "{} transition prefix no longer reaches its first line",
        target.record_id
    );
    let mut direct_line_starts = target
        .direct_lines
        .iter()
        .map(|line| line.file_offset)
        .collect::<BTreeSet<_>>();
    let mut transition_line_starts = target
        .transition_lines
        .iter()
        .map(|line| line.file_offset)
        .collect::<BTreeSet<_>>();
    direct_line_starts.insert(target.record_end_file_offset_exclusive);
    transition_line_starts.insert(target.record_end_file_offset_exclusive);
    let common_start = direct_line_starts
        .intersection(&transition_line_starts)
        .copied()
        .next()
        .with_context(|| {
            format!(
                "{} entry modes never reach a common line boundary",
                target.record_id
            )
        })?;
    ensure!(
        common_start >= direct_start && common_start >= transition_start,
        "{} common line boundary precedes an entry start",
        target.record_id,
    );
    let direct_literals = target
        .direct_lines
        .iter()
        .flat_map(|line| line.literal_file_offsets.iter().copied())
        .collect::<Vec<_>>();
    let transition_literals = target
        .transition_lines
        .iter()
        .flat_map(|line| line.literal_file_offsets.iter().copied())
        .collect::<Vec<_>>();
    let direct_common_literals = literal_offsets_in_range(
        &direct_literals,
        common_start,
        target.record_end_file_offset_exclusive,
    );
    let transition_common_literals = literal_offsets_in_range(
        &transition_literals,
        common_start,
        target.record_end_file_offset_exclusive,
    );
    ensure!(
        direct_common_literals == transition_common_literals,
        "{} entry modes disagree on common-body literal ownership",
        target.record_id
    );
    let direct_leading = build_leading_part(
        source,
        &format!("{}:direct-leading", target.record_id),
        EntryModePartRole::DirectLeading,
        direct_start,
        common_start,
        &direct_literals,
    )?;
    let transition_leading = build_leading_part(
        source,
        &format!("{}:transition-leading", target.record_id),
        EntryModePartRole::TransitionLeading,
        transition_start,
        common_start,
        &transition_literals,
    )?;
    let common_body = build_part(
        source,
        &format!("{}:common-body", target.record_id),
        EntryModePartRole::CommonBody,
        common_start,
        target.record_end_file_offset_exclusive,
        &direct_common_literals,
    )?;
    Ok(EntryModeRecord {
        id: target.record_id.clone(),
        incoming_transition_edge_count: target.incoming_transition_edge_count,
        direct_prefix_byte_count: target.direct_prefix_byte_count,
        transition_prefix_byte_count: target.transition_prefix_byte_count,
        common_body_source_file_offset_hex: format!("0x{common_start:05X}"),
        divergent_segment_source_sha1: sha1_hex(
            source
                .get(target.record_file_offset..common_start)
                .context("entry-mode divergent segment is outside the source")?,
        ),
        direct_leading,
        common_body,
        transition_leading,
    })
}

fn build_leading_part(
    source: &[u8],
    id: &str,
    role: EntryModePartRole,
    start: usize,
    end: usize,
    literal_offsets: &[usize],
) -> Result<EntryModePart> {
    let literal_offsets = literal_offsets_in_range(literal_offsets, start, end);
    build_part(source, id, role, start, end, &literal_offsets)
}

fn build_part(
    source: &[u8],
    id: &str,
    role: EntryModePartRole,
    start: usize,
    end: usize,
    literal_offsets: &[usize],
) -> Result<EntryModePart> {
    let storage = source
        .get(start..end)
        .with_context(|| format!("{id} source storage is outside the ROM"))?;
    Ok(EntryModePart {
        id: id.to_owned(),
        role,
        source_file_offset_hex: format!("0x{start:05X}"),
        source_storage_byte_count: storage.len(),
        source_storage_sha1: sha1_hex(storage),
        source_markup: decode_range_markup(source, start, end, literal_offsets)
            .with_context(|| format!("decode {id}"))?,
        japanese_source_byte_count: literal_offsets
            .iter()
            .filter_map(|offset| source.get(*offset))
            .filter(|code| is_japanese_text_code(**code))
            .count(),
        korean: String::new(),
        status: TranslationStatus::Untranslated,
    })
}

fn decode_range_markup(
    source: &[u8],
    start: usize,
    end: usize,
    literal_offsets: &[usize],
) -> Result<String> {
    let literal_offsets = literal_offsets.iter().copied().collect::<BTreeSet<_>>();
    let mut markup = String::new();
    let mut cursor = start;
    while cursor < end {
        let code = source[cursor];
        if literal_offsets.contains(&cursor) {
            append_literal_markup(&mut markup, code);
            cursor += 1;
            continue;
        }
        let control = DIALOGUE_CONTROL_SPECS
            .iter()
            .find(|control| control.code == code)
            .with_context(|| format!("structural byte {code:02X} is not a dialogue control"))?;
        let control_end = cursor
            .checked_add(
                1 + control.inline_operand_byte_count + control.transition_target_byte_count,
            )
            .context("entry-mode control range overflow")?;
        ensure!(
            control_end <= end,
            "entry-mode control crosses its part boundary"
        );
        markup.push('{');
        markup.push_str(&format!("{code:02X}"));
        for operand in &source[cursor + 1..control_end] {
            markup.push(':');
            markup.push_str(&format!("{operand:02X}"));
        }
        markup.push('}');
        cursor = control_end;
    }
    Ok(markup)
}

pub(super) fn seed_entry_mode_translations(
    record: &mut EntryModeRecord,
    main_record: &WorkspaceRecord,
) -> Result<()> {
    seed_part_translation(&mut record.transition_leading, main_record, true)?;
    seed_part_translation(&mut record.common_body, main_record, true)?;
    seed_part_translation(&mut record.direct_leading, main_record, false)?;
    Ok(())
}

fn seed_part_translation(
    part: &mut EntryModePart,
    main_record: &WorkspaceRecord,
    exact_line_boundary_required: bool,
) -> Result<()> {
    if part.source_storage_byte_count == 0 || part.japanese_source_byte_count == 0 {
        return Ok(());
    }
    let Some(start) = main_record
        .lines
        .iter()
        .position(|line| line.file_offset_hex == part.source_file_offset_hex)
    else {
        ensure!(
            !exact_line_boundary_required,
            "{} is not a line boundary in the transition-view workspace",
            part.id
        );
        return Ok(());
    };
    let mut end = None;
    let mut source_markup = String::new();
    for (relative_index, line) in main_record.lines[start..].iter().enumerate() {
        source_markup.push_str(&line.source_markup);
        if source_markup == part.source_markup {
            end = Some(start + relative_index + 1);
            break;
        }
        if !part.source_markup.starts_with(&source_markup) {
            break;
        }
    }
    let Some(end) = end else {
        ensure!(
            !exact_line_boundary_required,
            "{} does not match complete transition-view lines",
            part.id
        );
        return Ok(());
    };
    let source_lines = &main_record.lines[start..end];
    let mut status = TranslationStatus::Complete;
    let mut korean = String::new();
    for line in source_lines {
        if line.japanese_source_byte_count == 0 {
            korean.push_str(&line.source_markup);
            continue;
        }
        ensure!(
            line.status != TranslationStatus::Untranslated && !line.korean.is_empty(),
            "{} cannot reuse {} because it is untranslated",
            part.id,
            line.id
        );
        korean.push_str(&line.korean);
        status = least_reviewed_status(status, line.status);
    }
    part.korean = korean;
    part.status = status;
    Ok(())
}

fn least_reviewed_status(left: TranslationStatus, right: TranslationStatus) -> TranslationStatus {
    use TranslationStatus::*;
    match (left, right) {
        (Untranslated, _) | (_, Untranslated) => Untranslated,
        (InProgress, _) | (_, InProgress) => InProgress,
        (NeedsHumanReview, _) | (_, NeedsHumanReview) => NeedsHumanReview,
        (NeedsReview, _) | (_, NeedsReview) => NeedsReview,
        (Complete, Complete) => Complete,
    }
}

fn literal_offsets_in_range(offsets: &[usize], start: usize, end: usize) -> Vec<usize> {
    offsets
        .iter()
        .copied()
        .filter(|offset| (start..end).contains(offset))
        .collect()
}

fn differing_entry_start_japanese_byte_count(
    target: &crate::dialogue_inventory::MainDialogueTransitionTargetMode,
) -> Result<usize> {
    let bytes = match (
        target.direct_prefix_byte_count,
        target.transition_prefix_byte_count,
        target.transition_to_direct_body_delta,
    ) {
        (4, 0, 4) => &target.leading_source_bytes[..4],
        (4, 6, -2) => &target.leading_source_bytes[4..6],
        mode => anyhow::bail!("unsupported entry-mode prefix combination {mode:?}"),
    };
    Ok(bytes
        .iter()
        .filter(|code| is_japanese_text_code(**code))
        .count())
}
