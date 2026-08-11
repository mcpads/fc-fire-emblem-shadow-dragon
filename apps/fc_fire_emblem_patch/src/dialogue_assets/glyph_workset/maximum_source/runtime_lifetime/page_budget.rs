use std::collections::BTreeSet;

use anyhow::Result;

use crate::dialogue_assets::{
    LogicalDialogueByte, TranslationStatus, WorkspaceRecord, encode_korean_markup,
};

use super::DISPLAY_LINES_PER_PAGE;

pub(super) fn page_glyph_sets(
    record: &WorkspaceRecord,
) -> Result<(Vec<BTreeSet<char>>, Vec<BTreeSet<char>>)> {
    let mut filled_pages = Vec::new();
    let mut approved_pages = Vec::new();
    for page_lines in record.lines.chunks(DISPLAY_LINES_PER_PAGE) {
        let mut filled = BTreeSet::new();
        let mut approved = BTreeSet::new();
        for line in page_lines {
            if line.status == TranslationStatus::Untranslated {
                continue;
            }
            let glyphs = encode_korean_markup(&line.korean)?
                .into_iter()
                .filter_map(|byte| match byte {
                    LogicalDialogueByte::TargetGlyph(character) => Some(character),
                    LogicalDialogueByte::Encoded(_) => None,
                })
                .collect::<BTreeSet<_>>();
            filled.extend(glyphs.iter().copied());
            if line.status == TranslationStatus::Complete {
                approved.extend(glyphs);
            }
        }
        filled_pages.push(filled);
        approved_pages.push(approved);
    }
    Ok((filled_pages, approved_pages))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogue_assets::WorkspaceLine;

    #[test]
    fn page_partition_counts_the_largest_visible_four_line_glyph_set() {
        let record = record_with_lines(vec![
            line(0, "가나{ED}", TranslationStatus::Complete),
            line(1, "나다{ED}", TranslationStatus::Complete),
            line(2, "라마{ED}", TranslationStatus::Complete),
            line(3, "가마{ED}", TranslationStatus::Complete),
            line(4, "바사{EF}", TranslationStatus::NeedsHumanReview),
        ]);

        let (filled, approved) = page_glyph_sets(&record).unwrap();

        assert_eq!(filled.len(), 2);
        assert_eq!(filled[0].len(), 5);
        assert_eq!(filled[1].len(), 2);
        assert_eq!(approved[0].len(), 5);
        assert!(approved[1].is_empty());
    }

    fn record_with_lines(lines: Vec<WorkspaceLine>) -> WorkspaceRecord {
        WorkspaceRecord {
            id: "village-and-outro-dialogue:024".to_owned(),
            table_id: "village-and-outro-dialogue".to_owned(),
            source_prg_bank: 0x0C,
            canonical_entry_index: 24,
            entry_indices: vec![24],
            pointer_cpu_address_hex: "0x8000".to_owned(),
            prefix_byte_count: 0,
            boundary_control_hex: "EF".to_owned(),
            lines,
        }
    }

    fn line(index: usize, korean: &str, status: TranslationStatus) -> WorkspaceLine {
        WorkspaceLine {
            id: format!("line-{index}"),
            index,
            file_offset_hex: "0x00000".to_owned(),
            source_storage_sha1: "source".to_owned(),
            source_markup: if index == 4 {
                "あ{EF}".to_owned()
            } else {
                "あ{ED}".to_owned()
            },
            korean: korean.to_owned(),
            status,
            japanese_source_byte_count: 1,
            safe_japanese_source_byte_count: 1,
            requires_relocation: false,
            conflicting_file_offsets_hex: Vec::new(),
        }
    }
}
