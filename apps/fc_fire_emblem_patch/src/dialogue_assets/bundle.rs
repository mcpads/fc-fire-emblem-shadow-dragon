use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    ops::Range,
    path::Path,
};

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_inventory::inspect_main_dialogue_storage, font_slots::active_hangul_codes,
    japanese_encoding::is_japanese_text_code, rom::Rom, sha1_hex,
};

use super::*;

mod page_encoding;
mod region;
mod validation;

use page_encoding::visible_page_ranges;
use region::plan_region;
use validation::{validate_target_records, validate_transition_closure};

const DIALOGUE_PREFIX_CONTROL_CODE: u8 = 0xEA;
const DIALOGUE_PREFIX_OUTPUT_CODES: [u8; 2] = [0x9E, 0xAB];

pub(crate) struct MainDialogueBundlePlan {
    pub(crate) workspace_sha1: String,
    pub(crate) record_ids: Vec<String>,
    pub(crate) translated_line_count: usize,
    pub(crate) source_record_storage_byte_count: usize,
    pub(crate) planned_record_storage_byte_count: usize,
    pub(crate) preserved_source_codes: BTreeSet<u8>,
    pub(crate) source_reclaimable_active_codes: BTreeSet<u8>,
    pub(crate) page_worksets: Vec<MainDialoguePageWorkset>,
    pub(crate) line_layout: MainDialogueLineLayoutPlan,
    target_records: Vec<LogicalDialogueRecord>,
    visible_page_ranges_by_record_id: BTreeMap<String, Vec<Range<usize>>>,
    regions: Vec<LogicalBundleRegion>,
}

#[derive(Clone)]
pub(crate) struct MainDialoguePageWorkset {
    pub(crate) record_id: String,
    pub(crate) page_index: usize,
    pub(crate) target_glyphs: BTreeSet<char>,
    pub(crate) dynamic_string_selectors: BTreeSet<u8>,
    pub(crate) dynamic_string_selector_counts: BTreeMap<u8, usize>,
    pub(crate) dynamic_string_control_count: usize,
    pub(crate) source_reclaimable_active_codes: BTreeSet<u8>,
    pub(crate) preserved_target_active_codes: BTreeSet<u8>,
}

impl MainDialogueBundlePlan {
    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.target_records
            .iter()
            .flat_map(|record| &record.bytes)
            .filter_map(|byte| match byte {
                LogicalDialogueByte::TargetGlyph(glyph) => Some(*glyph),
                LogicalDialogueByte::Encoded(_) => None,
            })
            .collect()
    }

    pub(crate) fn encoded(
        &self,
        assignments: &BTreeMap<char, u8>,
    ) -> Result<EncodedMainDialogueBundle> {
        let regions = self
            .regions
            .iter()
            .map(|region| {
                Ok(EncodedMainDialogueRegion {
                    file_offset: region.file_offset,
                    source_storage: region.source_storage.clone(),
                    encoded_storage: encode_logical_bytes(&region.logical_storage, assignments)?,
                    used_storage_byte_count: region.used_storage_byte_count,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let pointer_writes = self
            .regions
            .iter()
            .flat_map(|region| region.pointer_writes.iter().cloned())
            .collect();
        Ok(EncodedMainDialogueBundle {
            regions,
            pointer_writes,
        })
    }
}

pub(crate) struct EncodedMainDialogueBundle {
    pub(crate) regions: Vec<EncodedMainDialogueRegion>,
    pub(crate) pointer_writes: Vec<MainDialoguePointerWrite>,
}

pub(crate) struct EncodedMainDialogueRegion {
    pub(crate) file_offset: usize,
    pub(crate) source_storage: Vec<u8>,
    pub(crate) encoded_storage: Vec<u8>,
    pub(crate) used_storage_byte_count: usize,
}

#[derive(Clone)]
pub(crate) struct MainDialoguePointerWrite {
    pub(crate) record_id: String,
    pub(crate) file_offset: usize,
    pub(crate) source_pointer: u16,
    pub(crate) planned_pointer: u16,
}

struct LogicalBundleRegion {
    file_offset: usize,
    source_prg_bank: u8,
    source_storage: Vec<u8>,
    logical_storage: Vec<LogicalDialogueByte>,
    logical_records: Vec<LogicalDialogueRecord>,
    used_storage_byte_count: usize,
    pointer_writes: Vec<MainDialoguePointerWrite>,
}

pub(crate) fn plan_main_dialogue_bundle(
    rom: &Rom,
    workspace_path: &Path,
    record_ids: &[&str],
) -> Result<MainDialogueBundlePlan> {
    rom.verify_supported_japanese()?;
    ensure!(
        !record_ids.is_empty(),
        "main dialogue bundle has no records"
    );
    let requested = record_ids.iter().copied().collect::<BTreeSet<_>>();
    ensure!(
        requested.len() == record_ids.len(),
        "main dialogue bundle contains duplicate record IDs"
    );
    let workspace_bytes = fs::read(workspace_path)
        .with_context(|| format!("read main dialogue workspace {}", workspace_path.display()))?;
    let workspace: MainDialogueWorkspace = serde_json::from_slice(&workspace_bytes)
        .with_context(|| format!("parse main dialogue workspace {}", workspace_path.display()))?;
    let expected = build_workspace(rom.data())?;
    validate_workspace_binding(&workspace, &expected)?;
    validate_workspace_translations(&workspace)?;

    let source_records = inspect_main_dialogue_storage(rom.data())?.records;
    ensure!(
        source_records.len() == workspace.records.len(),
        "main dialogue bundle lost workspace records"
    );
    let record_index_by_id = workspace
        .records
        .iter()
        .enumerate()
        .map(|(index, record)| (record.id.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        requested
            .iter()
            .all(|record_id| record_index_by_id.contains_key(record_id)),
        "main dialogue bundle contains an unknown record ID"
    );
    validate_target_records(&workspace, &source_records, &requested)?;
    let dialogue_graph = inspect_main_dialogue_graph(rom.data())?;
    validate_transition_closure(&dialogue_graph, &source_records, &requested)?;
    let line_layout = build_main_dialogue_line_layout_plan(
        rom.data(),
        &source_records,
        &workspace,
        &dialogue_graph,
        &requested,
    )?;

    let target_indices = requested
        .iter()
        .map(|record_id| record_index_by_id[record_id])
        .collect::<BTreeSet<_>>();
    let target_records = target_indices
        .iter()
        .map(|index| {
            build_logical_dialogue_record(
                rom.data(),
                &source_records[*index],
                &workspace.records[*index],
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let visible_page_ranges_by_record_id = target_indices
        .iter()
        .zip(&target_records)
        .map(|(index, record)| {
            Ok((
                record.id.clone(),
                visible_page_ranges(
                    &source_records[*index],
                    &workspace.records[*index],
                    record.bytes.len(),
                )?,
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let page_worksets = target_indices
        .iter()
        .flat_map(|index| {
            record_page_worksets(
                rom.data(),
                &source_records[*index],
                &workspace.records[*index],
            )
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        !page_worksets.is_empty(),
        "main dialogue bundle has no visible page worksets"
    );
    let target_regions = normalize_storage_ranges(
        &source_records
            .iter()
            .enumerate()
            .filter(|(index, _)| target_indices.contains(index))
            .map(|(_, record)| record.clone())
            .collect::<Vec<_>>(),
    )?;
    let owned_regions = normalize_storage_ranges(&source_records)?;
    let affected_regions = owned_regions
        .into_iter()
        .filter(|region| {
            target_regions.iter().any(|target| {
                region.source_prg_bank == target.source_prg_bank
                    && target.start < region.end_exclusive
                    && region.start < target.end_exclusive
            })
        })
        .collect::<Vec<_>>();
    ensure!(
        !affected_regions.is_empty(),
        "main dialogue bundle has no affected storage region"
    );

    let mut regions = Vec::new();
    for region in affected_regions {
        regions.push(plan_region(
            rom.data(),
            &source_records,
            &workspace.records,
            &target_indices,
            region,
        )?);
    }

    let translated_line_count = target_records
        .iter()
        .map(|record| record.translated_line_count)
        .sum();
    let source_record_storage_byte_count = target_indices
        .iter()
        .map(|index| source_records[*index].storage_byte_count)
        .sum();
    let planned_record_storage_byte_count =
        target_records.iter().map(|record| record.bytes.len()).sum();
    let mut preserved_source_codes = target_records
        .iter()
        .flat_map(|record| &record.bytes)
        .filter_map(|byte| match byte {
            LogicalDialogueByte::Encoded(value) => Some(*value),
            LogicalDialogueByte::TargetGlyph(_) => None,
        })
        .collect::<BTreeSet<_>>();
    if target_records.iter().any(|record| {
        record.bytes.iter().any(|byte| {
            matches!(
                byte,
                LogicalDialogueByte::Encoded(DIALOGUE_PREFIX_CONTROL_CODE)
            )
        })
    }) {
        preserved_source_codes.extend(DIALOGUE_PREFIX_OUTPUT_CODES);
    }
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let mut source_reclaimable_active_codes = BTreeSet::new();
    for index in &target_indices {
        for (source_line, workspace_line) in source_records[*index]
            .lines
            .iter()
            .zip(&workspace.records[*index].lines)
        {
            if workspace_line.status == TranslationStatus::Untranslated {
                continue;
            }
            for file_offset in &source_line.literal_file_offsets {
                let code = *rom
                    .data()
                    .get(*file_offset)
                    .context("main dialogue literal reclamation offset is outside the ROM")?;
                if is_japanese_text_code(code) && active_codes.contains(&code) {
                    source_reclaimable_active_codes.insert(code);
                }
            }
        }
    }
    source_reclaimable_active_codes.retain(|code| !preserved_source_codes.contains(code));
    ensure!(
        !source_reclaimable_active_codes.is_empty(),
        "main dialogue bundle has no exact source Japanese codes to reclaim"
    );

    Ok(MainDialogueBundlePlan {
        workspace_sha1: sha1_hex(&workspace_bytes),
        record_ids: record_ids.iter().map(|id| (*id).to_owned()).collect(),
        translated_line_count,
        source_record_storage_byte_count,
        planned_record_storage_byte_count,
        preserved_source_codes,
        source_reclaimable_active_codes,
        page_worksets,
        line_layout,
        target_records,
        visible_page_ranges_by_record_id,
        regions,
    })
}

pub(crate) fn plan_all_main_dialogue_records(
    rom: &Rom,
    workspace_path: &Path,
) -> Result<MainDialogueBundlePlan> {
    let workspace_bytes = fs::read(workspace_path)
        .with_context(|| format!("read main dialogue workspace {}", workspace_path.display()))?;
    let workspace: MainDialogueWorkspace = serde_json::from_slice(&workspace_bytes)
        .with_context(|| format!("parse main dialogue workspace {}", workspace_path.display()))?;
    ensure!(
        workspace.records.len() == 504,
        "all-record main dialogue installation must contain exactly 504 records"
    );
    let record_ids = workspace
        .records
        .iter()
        .map(|record| record.id.as_str())
        .collect::<Vec<_>>();
    let plan = plan_main_dialogue_bundle(rom, workspace_path, &record_ids)?;
    ensure!(
        plan.record_ids.len() == workspace.records.len(),
        "all-record main dialogue installation lost records"
    );
    Ok(plan)
}

fn record_page_worksets<'a>(
    source: &'a [u8],
    source_record: &'a MainDialogueStorageRecord,
    workspace_record: &'a WorkspaceRecord,
) -> impl Iterator<Item = Result<MainDialoguePageWorkset>> + 'a {
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    // 글꼴 타일로는 쓸 수 있어도 대사 바이트로 읽히면 명령이 되는 코드들이다.
    // 페이지마다 등장한 제어만 막으면 다른 페이지의 한글이 E4/E5/E6 같은 명령으로
    // 실행될 수 있으므로, 주 대사 코드북에서는 활성 제어 코드 전체를 예약한다.
    let script_control_codes = DIALOGUE_SCRIPT_CONTROL_CODES
        .into_iter()
        .filter(|code| active_codes.contains(code))
        .collect::<BTreeSet<_>>();
    let prefix_uses_dynamic_output = source
        .get(source_record.file_offset..source_record.file_offset + source_record.prefix_byte_count)
        .is_some_and(|prefix| prefix.contains(&DIALOGUE_PREFIX_CONTROL_CODE));
    source_record
        .lines
        .chunks(MAIN_DIALOGUE_VISIBLE_LINES_PER_PAGE)
        .zip(
            workspace_record
                .lines
                .chunks(MAIN_DIALOGUE_VISIBLE_LINES_PER_PAGE),
        )
        .enumerate()
        .map(move |(page_index, (source_lines, workspace_lines))| {
            ensure!(
                source_lines.len() == workspace_lines.len(),
                "{} visible-page source and workspace line counts differ",
                workspace_record.id
            );
            let mut target_glyphs = BTreeSet::new();
            let mut dynamic_string_selector_counts = BTreeMap::new();
            let mut dynamic_string_control_count = 0;
            let mut preserved_target_active_codes = script_control_codes.clone();
            let mut source_reclaimable_active_codes = BTreeSet::new();
            for (source_line, workspace_line) in source_lines.iter().zip(workspace_lines) {
                // EC/EA are runtime producers, not translated literals. Some records (notably
                // character epilogues) contain a control-only line whose Korean field is empty
                // and therefore remains `untranslated`. The source bytes are the authoritative
                // inventory for those controls; skipping the line would omit the produced name
                // glyphs from this page's font workset.
                let source_logical_line = source_line_logical_bytes(source, source_line)?;
                let line_selectors = dynamic_string_controls(&source_logical_line)?;
                let line_control_count = line_selectors.values().sum::<usize>();
                for (selector, count) in line_selectors {
                    *dynamic_string_selector_counts.entry(selector).or_default() += count;
                }
                dynamic_string_control_count += line_control_count;
                // 원문 줄에서 가져오는 것은 런타임이 합성하는 코드뿐이다. 원문 리터럴은
                // 번역문이 덮어쓰므로 대상 페이지에서 되찾을 수 있다.
                if source_logical_line
                    .contains(&LogicalDialogueByte::Encoded(DIALOGUE_PREFIX_CONTROL_CODE))
                {
                    preserved_target_active_codes.extend(DIALOGUE_PREFIX_OUTPUT_CODES);
                }

                if workspace_line.status == TranslationStatus::Untranslated {
                    continue;
                }
                let logical_line = encode_korean_markup(&workspace_line.korean)?;
                preserved_target_active_codes.extend(
                    rendered_active_codes(&logical_line)
                        .into_iter()
                        .filter(|code| active_codes.contains(code)),
                );
                for byte in logical_line {
                    if let LogicalDialogueByte::TargetGlyph(glyph) = byte {
                        target_glyphs.insert(glyph);
                    }
                }
                for file_offset in &source_line.literal_file_offsets {
                    let code = *source
                        .get(*file_offset)
                        .context("main-dialogue page literal is outside the ROM")?;
                    if is_japanese_text_code(code) && active_codes.contains(&code) {
                        source_reclaimable_active_codes.insert(code);
                    }
                }
            }
            if prefix_uses_dynamic_output {
                preserved_target_active_codes.extend(DIALOGUE_PREFIX_OUTPUT_CODES);
            }
            source_reclaimable_active_codes
                .retain(|code| !preserved_target_active_codes.contains(code));
            Ok(MainDialoguePageWorkset {
                record_id: workspace_record.id.clone(),
                page_index,
                target_glyphs,
                dynamic_string_selectors: dynamic_string_selector_counts.keys().copied().collect(),
                dynamic_string_selector_counts,
                dynamic_string_control_count,
                source_reclaimable_active_codes,
                preserved_target_active_codes,
            })
        })
}

fn source_line_logical_bytes(
    source: &[u8],
    source_line: &MainDialogueStorageLine,
) -> Result<Vec<LogicalDialogueByte>> {
    let end = source_line
        .file_offset
        .checked_add(source_line.storage_byte_count)
        .context("main-dialogue source line range overflow")?;
    Ok(source
        .get(source_line.file_offset..end)
        .context("main-dialogue source line is outside the ROM")?
        .iter()
        .copied()
        .map(LogicalDialogueByte::Encoded)
        .collect())
}

/// 대사 바이트 자체가 아니라 제어 코드의 실행 결과로 화면에 생기는 글리프 코드를
/// 현재 페이지에서 보호한다. `{EA}`는 원본 표식 두 타일을 `9E AB`로 출력한다.
/// 이 논리열이 실제로 네임테이블에 그릴 수 있는 활성 코드만 고른다.
///
/// 뱅크 `0A`의 줄 버퍼 writer는 여덟 곳이고, 리터럴 경로 `$8299`는 15개 제어를
/// 모두 분기로 걸러 낸 뒤에만 바이트를 쓴다. 따라서 제어 코드와 그 피연산자는
/// 타일이 되지 않는다. `{EA}`가 합성하는 `9E`·`AB`와 평범한 리터럴만 남는다.
/// 제어 코드 자체는 저장 바이트가 파서를 다시 지나므로 글리프에 배정할 수
/// 없지만, 그 제약은 코드북 쪽이 따로 소유한다.
fn rendered_active_codes(bytes: &[LogicalDialogueByte]) -> BTreeSet<u8> {
    let mut rendered = BTreeSet::new();
    let mut index = 0;
    while index < bytes.len() {
        let LogicalDialogueByte::Encoded(code) = bytes[index] else {
            index += 1;
            continue;
        };
        let Some(control) = DIALOGUE_CONTROL_SPECS
            .iter()
            .find(|control| control.code == code)
        else {
            rendered.insert(code);
            index += 1;
            continue;
        };
        if code == DIALOGUE_PREFIX_CONTROL_CODE {
            rendered.extend(DIALOGUE_PREFIX_OUTPUT_CODES);
        }
        index += 1 + control.inline_operand_byte_count + control.transition_target_byte_count;
    }
    rendered
}

pub(super) fn dynamic_string_controls(
    bytes: &[LogicalDialogueByte],
) -> Result<BTreeMap<u8, usize>> {
    let mut selectors = BTreeMap::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != LogicalDialogueByte::Encoded(0xEC) {
            index += 1;
            continue;
        }
        let selector = match bytes.get(index + 1) {
            Some(LogicalDialogueByte::Encoded(selector)) if *selector <= 3 => *selector,
            _ => anyhow::bail!("main-dialogue EC control lost its selector operand"),
        };
        *selectors.entry(selector).or_default() += 1;
        index += 2;
    }
    Ok(selectors)
}

fn encode_logical_bytes(
    bytes: &[LogicalDialogueByte],
    assignments: &BTreeMap<char, u8>,
) -> Result<Vec<u8>> {
    bytes
        .iter()
        .map(|byte| match byte {
            LogicalDialogueByte::Encoded(value) => Ok(*value),
            LogicalDialogueByte::TargetGlyph(glyph) => assignments
                .get(glyph)
                .copied()
                .with_context(|| format!("missing main-dialogue bundle code for {glyph:?}")),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_control_and_its_operands_render_nothing() {
        // {E4:B1:2D} is one transition control plus two target bytes the handler
        // reads without advancing the current pointer. None reaches the line buffer.
        let line = [
            LogicalDialogueByte::Encoded(0xE4),
            LogicalDialogueByte::Encoded(0xB1),
            LogicalDialogueByte::Encoded(0x2D),
        ];

        assert!(rendered_active_codes(&line).is_empty());
    }

    #[test]
    fn an_inline_operand_renders_nothing() {
        // {DF:55} selects a bit table; {E9:02} sets the line count.
        let line = [
            LogicalDialogueByte::Encoded(0xDF),
            LogicalDialogueByte::Encoded(0x55),
            LogicalDialogueByte::Encoded(0xE9),
            LogicalDialogueByte::Encoded(0x02),
        ];

        assert!(rendered_active_codes(&line).is_empty());
    }

    #[test]
    fn a_literal_byte_renders_its_own_code() {
        let line = [
            LogicalDialogueByte::Encoded(0x9C),
            LogicalDialogueByte::TargetGlyph('가'),
        ];

        assert_eq!(rendered_active_codes(&line), BTreeSet::from([0x9C]));
    }

    #[test]
    fn the_speaker_prefix_control_renders_the_two_codes_it_synthesizes() {
        let line = [LogicalDialogueByte::Encoded(DIALOGUE_PREFIX_CONTROL_CODE)];

        assert_eq!(
            rendered_active_codes(&line),
            DIALOGUE_PREFIX_OUTPUT_CODES.into_iter().collect()
        );
    }

    fn source_record_with_line(source: &[u8]) -> MainDialogueStorageRecord {
        MainDialogueStorageRecord {
            table_id: "synthetic-dialogue",
            source_prg_bank: 0,
            canonical_entry_index: 0,
            entry_indices: vec![0],
            pointer_file_offsets: vec![0],
            pointer_cpu_address: 0x8000,
            file_offset: 0,
            end_file_offset_exclusive: source.len(),
            storage_byte_count: source.len(),
            storage_sha1: String::new(),
            prefix_byte_count: 0,
            boundary_control: 0xED,
            literal_file_offsets: Vec::new(),
            lines: vec![MainDialogueStorageLine {
                file_offset: 0,
                storage_byte_count: source.len(),
                storage_sha1: String::new(),
                line_end_control: 0xED,
                literal_file_offsets: Vec::new(),
            }],
        }
    }

    fn workspace_record_with_line(status: TranslationStatus, korean: &str) -> WorkspaceRecord {
        WorkspaceRecord {
            id: "synthetic-dialogue:000".to_owned(),
            table_id: "synthetic-dialogue".to_owned(),
            source_prg_bank: 0,
            canonical_entry_index: 0,
            entry_indices: vec![0],
            pointer_cpu_address_hex: "8000".to_owned(),
            prefix_byte_count: 0,
            boundary_control_hex: "ED".to_owned(),
            lines: vec![WorkspaceLine {
                id: "synthetic-dialogue:000:line:00".to_owned(),
                index: 0,
                file_offset_hex: "0x00000".to_owned(),
                source_storage_sha1: String::new(),
                source_markup: "{E2}{E9:05}{EC:00}{ED}".to_owned(),
                korean: korean.to_owned(),
                status,
                japanese_source_byte_count: 0,
                safe_japanese_source_byte_count: 0,
                requires_relocation: false,
                conflicting_file_offsets_hex: Vec::new(),
            }],
        }
    }

    #[test]
    fn dynamic_string_inventory_counts_controls_and_unique_selectors() {
        let logical = encode_korean_markup("{EC:00}한{EC:00}{EC:02}{EF}").unwrap();

        let selectors = dynamic_string_controls(&logical).unwrap();

        assert_eq!(selectors, BTreeMap::from([(0, 2), (2, 1)]));
    }

    #[test]
    fn a_page_workset_does_not_preserve_the_japanese_literals_it_replaces() {
        // 원문 리터럴 08은 번역문이 덮어쓰는 코드라 대상 페이지에서 되찾을 수 있어야
        // 한다. 같은 줄의 {EA}가 합성하는 9E·AB만 남는다.
        let source = [0xEA, 0x08, 0x09, 0xED];
        let source_record = source_record_with_line(&source);
        let workspace_record =
            workspace_record_with_line(TranslationStatus::NeedsHumanReview, "{EA}가나{ED}");

        let worksets = record_page_worksets(&source, &source_record, &workspace_record)
            .collect::<Result<Vec<_>>>()
            .unwrap();

        let preserved = &worksets[0].preserved_target_active_codes;
        assert!(preserved.contains(&0x9E) && preserved.contains(&0xAB));
        assert!(!preserved.contains(&0x08));
        assert!(!preserved.contains(&0x09));
    }

    #[test]
    fn a_page_workset_preserves_controls_and_rendered_literals_but_not_operands() {
        let source = [0xDF, 0x55, 0xEA, 0x00, 0xE4, 0xB1, 0x2D];
        let source_record = source_record_with_line(&source);
        let workspace_record = workspace_record_with_line(
            TranslationStatus::NeedsHumanReview,
            "{DF:55}{EA}가{LIT:9C}{E4:B1:2D}",
        );

        let worksets = record_page_worksets(&source, &source_record, &workspace_record)
            .collect::<Result<Vec<_>>>()
            .unwrap();

        let preserved = &worksets[0].preserved_target_active_codes;
        // 그려지는 것: {EA}가 합성하는 9E·AB와 리터럴 9C.
        assert!(preserved.contains(&0x9E) && preserved.contains(&0xAB));
        assert!(preserved.contains(&0x9C));
        // 파서가 먼저 보는 제어 코드는 글리프에 줄 수 없다.
        assert!(preserved.contains(&0xDF) && preserved.contains(&0xE4));
        // 피연산자는 그려지지도 파싱되지도 않으므로 슬롯을 묶지 않는다.
        assert!(!preserved.contains(&0x55));
        assert!(!preserved.contains(&0xB1));
        assert!(!preserved.contains(&0x2D));
    }

    #[test]
    fn untranslated_control_only_line_still_populates_dynamic_name_workset() {
        let source = [0xE2, 0xE9, 0x05, 0xEC, 0x00, 0xED];
        let source_record = source_record_with_line(&source);
        let workspace_record = workspace_record_with_line(TranslationStatus::Untranslated, "");

        let worksets = record_page_worksets(&source, &source_record, &workspace_record)
            .collect::<Result<Vec<_>>>()
            .unwrap();

        assert_eq!(worksets.len(), 1);
        assert_eq!(worksets[0].dynamic_string_control_count, 1);
        assert_eq!(
            worksets[0].dynamic_string_selector_counts,
            BTreeMap::from([(0, 1)])
        );
        assert_eq!(worksets[0].dynamic_string_selectors, BTreeSet::from([0]));
        assert!(worksets[0].target_glyphs.is_empty());
    }

    #[test]
    fn translated_dynamic_control_is_counted_once_from_its_source_contract() {
        let source = [0xE2, 0xE9, 0x05, 0xEC, 0x00, 0xED];
        let source_record = source_record_with_line(&source);
        let workspace_record =
            workspace_record_with_line(TranslationStatus::Complete, "{E2}{E9:05}{EC:00}{ED}");

        let worksets = record_page_worksets(&source, &source_record, &workspace_record)
            .collect::<Result<Vec<_>>>()
            .unwrap();

        assert_eq!(worksets[0].dynamic_string_control_count, 1);
        assert_eq!(
            worksets[0].dynamic_string_selector_counts,
            BTreeMap::from([(0, 1)])
        );
    }
}
