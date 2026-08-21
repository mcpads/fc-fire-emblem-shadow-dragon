use super::*;

pub(super) fn validate_translation_markup(line: &WorkspaceLine) -> Result<usize> {
    validate_translation_markup_pair(&line.id, &line.source_markup, &line.korean, true)
}

pub(super) fn validate_translation_markup_pair(
    id: &str,
    source_markup: &str,
    korean_markup: &str,
    requires_final_control: bool,
) -> Result<usize> {
    let source = inspect_markup(source_markup, MarkupRole::Source)
        .with_context(|| format!("inspect protected source markup at {id}"))?;
    let target = inspect_markup(korean_markup, MarkupRole::KoreanTarget)
        .with_context(|| format!("inspect korean markup at {id}"))?;
    ensure!(
        preserves_main_dialogue_protected_items(&source.protected_items, &target.protected_items,),
        "{id} changed, removed, or added a protected control token or existing English character"
    );
    validate_neutral_variable_postpositions(id, korean_markup, |_, _| false)?;
    if requires_final_control {
        let final_control = source
            .protected_items
            .last()
            .filter(|item| item.starts_with('{'))
            .context("source line does not end in a protected control token")?;
        ensure!(
            korean_markup.ends_with(final_control),
            "{id} must keep its line-end control token at the end"
        );
    }
    Ok(target.editable_glyph_count)
}

const EDITABLE_JAPANESE_PRESENTATION_TOKENS: [&str; 5] =
    ["{LIT:96}", "{LIT:99}", "{LIT:9A}", "{LIT:BD}", "{LIT:BE}"];

pub(super) fn preserves_main_dialogue_protected_items(
    source: &[String],
    target: &[String],
) -> bool {
    let structural_source = source
        .iter()
        .filter(|item| !EDITABLE_JAPANESE_PRESENTATION_TOKENS.contains(&item.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    preserves_protected_items_with_added_layout_spaces(&structural_source, target, "{SP}")
}

pub(super) fn preserves_protected_items_with_added_layout_spaces(
    source: &[String],
    target: &[String],
    layout_space: &str,
) -> bool {
    let mut source_index = 0;
    for target_item in target {
        if source
            .get(source_index)
            .is_some_and(|source_item| source_item == target_item)
        {
            source_index += 1;
        } else if target_item != layout_space {
            return false;
        }
    }
    source_index == source.len()
}

pub(super) fn preserves_battle_protected_items(source: &[String], target: &[String]) -> bool {
    let source_without_editable_periods = source
        .iter()
        .filter(|item| item.as_str() != ".")
        .cloned()
        .collect::<Vec<_>>();
    let target_without_editable_periods = target
        .iter()
        .filter(|item| item.as_str() != ".")
        .cloned()
        .collect::<Vec<_>>();
    preserves_protected_items_with_added_layout_spaces(
        &source_without_editable_periods,
        &target_without_editable_periods,
        "{SP}",
    )
}

pub(super) fn validate_neutral_variable_postpositions(
    line_id: &str,
    markup: &str,
    allow_bare_postposition: impl Fn(&str, &str) -> bool,
) -> Result<()> {
    let mut remaining = markup;
    while let Some(token_start) = remaining.find("{EC:") {
        let token = &remaining[token_start..];
        let token_end = token
            .find('}')
            .context("dynamic value token has no closing brace")?;
        let dynamic_value = &token[..=token_end];
        let suffix = &token[token_end + 1..];
        let grammatical_suffix = suffix.strip_prefix("{SP}").unwrap_or(suffix);
        if ["은(는)", "이(가)", "을(를)", "과(와)", "(으)로", "(이)나"]
            .iter()
            .any(|neutral| grammatical_suffix.starts_with(neutral))
        {
            remaining = suffix;
            continue;
        }
        let bare_postposition = [
            "으로", "이나", "은", "는", "이", "가", "을", "를", "과", "와", "로",
        ]
        .into_iter()
        .find(|postposition| grammatical_suffix.starts_with(postposition));
        if let Some(bare_postposition) = bare_postposition {
            ensure!(
                allow_bare_postposition(dynamic_value, bare_postposition),
                "{line_id} attaches bare postposition {bare_postposition:?} to variable value {dynamic_value}; use a neutral form"
            );
        }
        remaining = suffix;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MarkupRole {
    Source,
    KoreanTarget,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct MarkupInspection {
    pub(super) protected_items: Vec<String>,
    pub(super) editable_glyph_count: usize,
}

pub(super) fn inspect_markup(markup: &str, role: MarkupRole) -> Result<MarkupInspection> {
    let mut protected_items = Vec::new();
    let mut editable_glyph_count = 0;
    let mut chars = markup.char_indices().peekable();
    while let Some((start, character)) = chars.next() {
        if character == '{' {
            let end = chars
                .by_ref()
                .find_map(|(index, candidate)| (candidate == '}').then_some(index))
                .context("markup token has no closing brace")?;
            let token = &markup[start..=end];
            ensure!(
                !token[1..token.len() - 1].contains(['{', '}']),
                "markup token contains a nested brace"
            );
            if role == MarkupRole::KoreanTarget && is_target_punctuation_token(token) {
                editable_glyph_count += 1;
                continue;
            }
            protected_items.push(token.to_owned());
            continue;
        }
        ensure!(
            character != '}',
            "markup contains a closing brace without an opening brace"
        );

        if character.is_ascii_uppercase()
            || character.is_ascii_digit()
            || matches!(character, ':' | '.')
        {
            protected_items.push(character.to_string());
            continue;
        }

        match role {
            MarkupRole::Source => ensure!(
                is_japanese_markup_character(character),
                "source markup contains an unclassified character {character:?}"
            ),
            MarkupRole::KoreanTarget => {
                ensure!(
                    !is_japanese_markup_character(character),
                    "korean markup still contains Japanese character {character:?}"
                );
                ensure!(
                    is_korean_target_character(character),
                    "korean markup contains unsupported character {character:?}"
                );
                editable_glyph_count += 1;
            }
        }
    }
    Ok(MarkupInspection {
        protected_items,
        editable_glyph_count,
    })
}

fn is_target_punctuation_token(token: &str) -> bool {
    matches!(token, "{PUNCT:8E}" | "{PUNCT:8F}")
}

pub(super) fn is_japanese_markup_character(character: char) -> bool {
    (0..=u8::MAX)
        .any(|code| japanese_text_glyph(code).is_some_and(|glyph| glyph.starts_with(character)))
}

pub(super) fn is_korean_target_character(character: char) -> bool {
    matches!(character, '\u{AC00}'..='\u{D7A3}') || KOREAN_TARGET_PUNCTUATION.contains(&character)
}

pub(super) const KOREAN_TARGET_PUNCTUATION: [char; 8] = [',', '!', '?', '~', '-', '\'', '(', ')'];

pub(super) fn build_logical_dialogue_record(
    source: &[u8],
    source_record: &MainDialogueStorageRecord,
    workspace_record: &WorkspaceRecord,
) -> Result<LogicalDialogueRecord> {
    ensure!(
        workspace_record.id
            == format!(
                "{}:{:03}",
                source_record.table_id, source_record.canonical_entry_index
            ),
        "main dialogue logical record binding changed"
    );
    ensure!(
        source_record.lines.len() == workspace_record.lines.len(),
        "{} logical line coverage changed",
        workspace_record.id
    );
    ensure!(
        source_record.pointer_file_offsets.len() == source_record.entry_indices.len(),
        "{} pointer write coverage changed",
        workspace_record.id
    );
    for pointer_file_offset in &source_record.pointer_file_offsets {
        ensure!(
            source.get(*pointer_file_offset..*pointer_file_offset + 2)
                == Some(&source_record.pointer_cpu_address.to_le_bytes()),
            "{} pointer table source bytes changed at 0x{pointer_file_offset:05X}",
            workspace_record.id
        );
    }

    let prefix_end = source_record
        .file_offset
        .checked_add(source_record.prefix_byte_count)
        .context("main dialogue record prefix range overflow")?;
    let mut bytes = source
        .get(source_record.file_offset..prefix_end)
        .with_context(|| format!("{} prefix is outside the source", workspace_record.id))?
        .iter()
        .copied()
        .map(LogicalDialogueByte::Encoded)
        .collect::<Vec<_>>();
    let mut source_cursor = prefix_end;
    let mut translated_line_count = 0;
    for (source_line, workspace_line) in source_record.lines.iter().zip(&workspace_record.lines) {
        ensure!(
            source_line.file_offset == source_cursor,
            "{} source lines are not contiguous",
            workspace_record.id
        );
        let source_line_end = source_line
            .file_offset
            .checked_add(source_line.storage_byte_count)
            .context("main dialogue source line range overflow")?;
        if workspace_line.status == TranslationStatus::Untranslated {
            bytes.extend(
                source
                    .get(source_line.file_offset..source_line_end)
                    .with_context(|| {
                        format!("{} source storage is outside the ROM", workspace_line.id)
                    })?
                    .iter()
                    .copied()
                    .map(LogicalDialogueByte::Encoded),
            );
        } else {
            translated_line_count += 1;
            bytes.extend(
                encode_korean_markup(&workspace_line.korean)
                    .with_context(|| format!("encode logical markup at {}", workspace_line.id))?,
            );
        }
        source_cursor = source_line_end;
    }
    ensure!(
        source_cursor == source_record.end_file_offset_exclusive,
        "{} logical record did not consume its exact source range",
        workspace_record.id
    );

    Ok(LogicalDialogueRecord {
        id: workspace_record.id.clone(),
        source_prg_bank: source_record.source_prg_bank,
        source_pointer_cpu_address: source_record.pointer_cpu_address,
        pointer_file_offsets: source_record.pointer_file_offsets.clone(),
        source_file_offset: source_record.file_offset,
        source_storage_byte_count: source_record.storage_byte_count,
        translated_line_count,
        bytes,
    })
}

pub(super) fn encode_korean_markup(markup: &str) -> Result<Vec<LogicalDialogueByte>> {
    let mut encoded = Vec::new();
    let mut chars = markup.char_indices().peekable();
    while let Some((start, character)) = chars.next() {
        if character == '{' {
            let end = chars
                .by_ref()
                .find_map(|(index, candidate)| (candidate == '}').then_some(index))
                .context("markup token has no closing brace")?;
            encoded.extend(
                decode_protected_token(&markup[start..=end])?
                    .into_iter()
                    .map(LogicalDialogueByte::Encoded),
            );
            continue;
        }
        ensure!(character != '}', "markup has an unmatched closing brace");
        if let Some(code) = encode_protected_literal(character) {
            encoded.push(LogicalDialogueByte::Encoded(code));
        } else {
            ensure!(
                is_korean_target_character(character),
                "unsupported korean target character {character:?}"
            );
            encoded.push(LogicalDialogueByte::TargetGlyph(character));
        }
    }
    Ok(encoded)
}

pub(super) fn encode_protected_literal(character: char) -> Option<u8> {
    match character {
        '0'..='9' => Some(0x60 + (character as u8 - b'0')),
        'A'..='Z' => Some(0x6A + (character as u8 - b'A')),
        ':' => Some(0x8D),
        '.' => Some(0x9B),
        _ => None,
    }
}

pub(super) fn decode_protected_token(token: &str) -> Result<Vec<u8>> {
    ensure!(
        token.starts_with('{') && token.ends_with('}'),
        "protected token is missing braces"
    );
    let body = &token[1..token.len() - 1];
    if body == "SP" {
        return Ok(vec![0xFF]);
    }
    if matches!(body, "AB" | "AC") {
        return Ok(vec![decode_hex_byte(body)?]);
    }
    if let Some(punctuation) = body.strip_prefix("PUNCT:") {
        let code = decode_hex_byte(punctuation)?;
        ensure!(
            matches!(code, 0x8E | 0x8F),
            "target punctuation code must be 8E or 8F"
        );
        return Ok(vec![code]);
    }
    if let Some(literal) = body.strip_prefix("LIT:") {
        return Ok(vec![decode_hex_byte(literal)?]);
    }
    let bytes = body
        .split(':')
        .map(decode_hex_byte)
        .collect::<Result<Vec<_>>>()?;
    let (control_code, operands) = bytes
        .split_first()
        .context("protected control token is empty")?;
    let control = DIALOGUE_CONTROL_SPECS
        .iter()
        .find(|control| control.code == *control_code)
        .with_context(|| format!("unknown dialogue control {control_code:02X}"))?;
    let expected_operand_count =
        control.inline_operand_byte_count + control.transition_target_byte_count;
    ensure!(
        operands.len() == expected_operand_count,
        "dialogue control {control_code:02X} requires {expected_operand_count} stored operands, found {}",
        operands.len()
    );
    Ok(bytes)
}

pub(super) fn decode_hex_byte(encoded: &str) -> Result<u8> {
    ensure!(
        encoded.len() == 2 && encoded.is_ascii(),
        "hex byte must contain exactly two ASCII digits"
    );
    let digits = encoded.as_bytes();
    Ok((decode_hex_digit(digits[0])? << 4) | decode_hex_digit(digits[1])?)
}
