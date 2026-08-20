use std::collections::BTreeSet;
use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    font_slots::active_hangul_codes,
    rom::{EXPECTED_SOURCE_SHA1, HEADER_SIZE, PRG_SIZE, Rom},
    sha1_hex,
    translation_consumer::{
        ScreenConsumerSourceBinding, TranslationConsumerSourceEvidence,
        qualified_source_binding_id, source_binding_id,
    },
};

mod command_menu;
mod glyph_budget;
mod source_spec;
mod target_glyphs;
#[cfg(test)]
mod tests;
mod workspace;

pub(crate) use command_menu::{
    COMMAND_LABEL_SPECS, MapFacilityDispatchSource, bind_map_facility_dispatch_source,
};
use source_spec::*;
pub(crate) use source_spec::{
    FixedLabelSpec, SUMMARY_AND_STATUS_LABEL_SPECS, composite_payload_display_cell_count,
    terminated_composite_display_cell_count,
};
pub(crate) use workspace::plan_unit_ui_labels;

pub(crate) fn translated_fixed_string_indices() -> BTreeSet<u8> {
    SUMMARY_AND_STATUS_LABEL_SPECS
        .iter()
        .chain(command_menu::COMMAND_LABEL_SPECS)
        .filter(|spec| spec.translation_scope == "japanese_only")
        .map(|spec| spec.index)
        .collect()
}

const PRG_BANK_SIZE: usize = 16 * 1024;
pub(super) const UNIT_UI_BANK: usize = 0x0B;
const SWITCHABLE_CPU_BASE: u16 = 0x8000;
const FIXED_STRING_POINTER_TABLE_ADDRESS: u16 = 0x8FC2;

#[derive(Debug, Serialize)]
struct UnitUiTextReport {
    schema: u8,
    source_sha1: &'static str,
    screen_roles: Vec<ScreenRoleReport>,
    composition_dispatch: CompositionDispatchReport,
    shared_appenders: Vec<CodeRegionReport>,
    fixed_labels: Vec<FixedLabelReport>,
    dynamic_sources: Vec<DynamicSourceReport>,
    glyph_budget: glyph_budget::GlyphBudgetReport,
    command_menu: command_menu::CommandMenuReport,
    page_lifetime: PageLifetimeReport,
    implementation_boundary: ImplementationBoundary,
}

#[derive(Debug, Serialize)]
struct ScreenRoleReport {
    screen_role: &'static str,
    composers: Vec<&'static str>,
    inherited_content: Vec<&'static str>,
    translation_scope: &'static str,
}

#[derive(Debug, Serialize)]
struct CompositionDispatchReport {
    state_address: &'static str,
    dispatcher: CodeRegionReport,
    relevant_states: Vec<CompositionStateReport>,
}

#[derive(Debug, Serialize)]
struct CompositionStateReport {
    state: u8,
    state_hex: String,
    role: &'static str,
    handler_address: u16,
    handler_address_hex: String,
    handler: CodeRegionReport,
}

#[derive(Debug, Serialize, Clone)]
pub(super) struct CodeRegionReport {
    pub(super) role: &'static str,
    pub(super) prg_bank: usize,
    pub(super) prg_bank_hex: String,
    pub(super) cpu_address: u16,
    pub(super) cpu_address_hex: String,
    pub(super) file_offset: usize,
    pub(super) file_offset_hex: String,
    pub(super) byte_count: usize,
    pub(super) bytes_hex: String,
}

#[derive(Debug, Serialize)]
struct FixedLabelReport {
    index: u8,
    index_hex: String,
    source_text: &'static str,
    translation_scope: &'static str,
    pointer_table_address: u16,
    pointer_table_address_hex: String,
    source_address: u16,
    source_address_hex: String,
    source_storage_byte_count: usize,
    source_display_cell_count: usize,
    source_codes: Vec<u8>,
    source_codes_hex: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DynamicSourceReport {
    role: &'static str,
    text_inventory_table_id: &'static str,
    selected_by: &'static str,
    destination: &'static str,
}

#[derive(Debug, Serialize)]
struct PageLifetimeReport {
    right_fd_page_supplied_by_screen_roles: Vec<&'static str>,
    proven_inherited_by_screen_roles: Vec<&'static str>,
    phase_state_address: &'static str,
    phase_dispatcher: CodeRegionReport,
    right_fd_page_supply: CodeRegionReport,
    runtime_evidence: &'static str,
    runtime_variant_coverage: &'static str,
}

#[derive(Debug, Serialize)]
struct ImplementationBoundary {
    required_design: &'static str,
    rejected_shortcut: &'static str,
    preserved_original: [&'static str; 3],
    separate_screen_contract: &'static str,
}

pub struct UnitUiTextSummary {
    pub report_sha1: String,
    pub screen_role_count: usize,
    pub composer_count: usize,
    pub fixed_label_count: usize,
    pub translated_japanese_label_count: usize,
    pub command_label_count: usize,
    pub dynamic_pointer_count: usize,
    pub dynamic_unique_string_count: usize,
    pub provisional_hangul_slot_ceiling: usize,
    pub single_family_page_fit: bool,
}

pub fn analyze_unit_ui_text(
    source_path: &Path,
    fixed_text_workspace_path: &Path,
    unit_name_workspace_path: &Path,
    unit_ui_label_workspace_path: &Path,
    report_path: &Path,
) -> Result<UnitUiTextSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let fixed_japanese_label_count = SUMMARY_AND_STATUS_LABEL_SPECS
        .iter()
        .chain(command_menu::COMMAND_LABEL_SPECS)
        .filter(|label| label.translation_scope == "japanese_only")
        .count();
    let target_glyph_budget = target_glyphs::plan_target_glyph_budget(
        &source_rom,
        fixed_text_workspace_path,
        unit_name_workspace_path,
        unit_ui_label_workspace_path,
    )?;
    let glyph_budget = glyph_budget::analyze(
        source_rom.data(),
        fixed_japanese_label_count,
        target_glyph_budget,
    )?;
    let report = build_report(source_rom.prg(), glyph_budget)?;
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize unit-UI text report")?;
    let report_sha1 = sha1_hex(&report_bytes);

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(UnitUiTextSummary {
        report_sha1,
        screen_role_count: report.screen_roles.len(),
        composer_count: report.composition_dispatch.relevant_states.len(),
        fixed_label_count: report.fixed_labels.len(),
        translated_japanese_label_count: report
            .fixed_labels
            .iter()
            .filter(|label| label.translation_scope == "japanese_only")
            .count(),
        command_label_count: report.command_menu.static_label_count,
        dynamic_pointer_count: report.glyph_budget.dynamic_pointer_count(),
        dynamic_unique_string_count: report.glyph_budget.dynamic_unique_string_count(),
        provisional_hangul_slot_ceiling: report.glyph_budget.provisional_hangul_slot_ceiling(),
        single_family_page_fit: report.glyph_budget.single_family_page_fit(),
    })
}

pub(crate) fn inspect_unit_ui_japanese_label_count(source: &[u8]) -> Result<usize> {
    let prg = source
        .get(HEADER_SIZE..HEADER_SIZE + PRG_SIZE)
        .context("supported source does not contain the complete PRG region")?;
    validate_code_regions(prg)?;
    let fixed_labels = validate_fixed_labels(
        prg,
        SUMMARY_AND_STATUS_LABEL_SPECS
            .iter()
            .chain(command_menu::COMMAND_LABEL_SPECS),
    )?;
    Ok(fixed_labels
        .iter()
        .filter(|label| label.translation_scope == "japanese_only")
        .count())
}

/// 유닛 UI 생산자가 문자열 표를 거치지 않고 화면용 버퍼에 직접 쓰는 원본 글리프
/// 코드다. 합성 문자열뿐 아니라 아이템 슬롯 표식도 번역 논리 바이트에 나타나지
/// 않으므로 소비자 글꼴 페이지가 별도로 보존해야 한다.
pub(crate) fn preserved_unit_ui_display_codes(source: &[u8]) -> Result<BTreeSet<u8>> {
    let prg = source
        .get(HEADER_SIZE..HEADER_SIZE + PRG_SIZE)
        .context("supported source does not contain the complete PRG region")?;
    validate_code_regions(prg)?;

    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let preserved = CODE_REGION_SPECS
        .iter()
        .flat_map(|spec| spec.expected.windows(5))
        .filter_map(|window| {
            let writes_composite_buffer = window[2..] == [0x9D, 0x51, 0x04];
            let writes_item_marker_buffer = window[2..] == [0x99, 0xC8, 0x04];
            (window[0] == 0xA9 && (writes_composite_buffer || writes_item_marker_buffer))
                .then_some(window[1])
        })
        .filter(|code| active_codes.contains(code))
        .collect::<BTreeSet<_>>();
    ensure!(
        preserved == BTreeSet::from([0xAD, 0xAF, 0xBF]),
        "unit-UI direct display-code contract changed"
    );
    Ok(preserved)
}

pub(crate) fn preserved_codes_for_unit_name_projection(source: &[u8]) -> Result<BTreeSet<u8>> {
    let prg = source
        .get(HEADER_SIZE..HEADER_SIZE + PRG_SIZE)
        .context("supported source does not contain the complete PRG region")?;
    validate_code_regions(prg)?;
    validate_fixed_labels(
        prg,
        SUMMARY_AND_STATUS_LABEL_SPECS
            .iter()
            .chain(command_menu::COMMAND_LABEL_SPECS),
    )?;
    let mut preserved = crate::text_inventory::scoped_text_table_budgets(
        source,
        &["class-names", "item-names", "enemy-names"],
    )?
    .into_iter()
    .flat_map(|table| table.source_codes)
    .collect::<BTreeSet<_>>();
    preserved.extend(
        SUMMARY_AND_STATUS_LABEL_SPECS
            .iter()
            .chain(command_menu::COMMAND_LABEL_SPECS)
            .flat_map(|label| label.expected.iter().copied()),
    );
    preserved.extend(preserved_unit_ui_display_codes(source)?);
    Ok(preserved)
}

/// 요약·상태 화면이 고정 문자열 표에서 읽는 번역 대상 ID다. 소비자 코드북이 ID를
/// 다시 적어 두면 원본 표의 인덱스가 바뀌었을 때 조용히 다른 문구를 묶게 된다.
pub(crate) fn summary_and_status_label_ids() -> Vec<String> {
    SUMMARY_AND_STATUS_LABEL_SPECS
        .iter()
        .filter(|spec| spec.translation_scope == "japanese_only")
        .map(|spec| format!("unit-ui-label:{:02X}", spec.index))
        .collect()
}

/// 명령 메뉴가 고정 문자열 표에서 읽는 번역 대상 ID다.
pub(crate) fn command_menu_label_ids() -> Vec<String> {
    command_menu::COMMAND_LABEL_SPECS
        .iter()
        .filter(|spec| spec.translation_scope == "japanese_only")
        .map(|spec| format!("unit-ui-label:{:02X}", spec.index))
        .collect()
}

/// 요약이 이름 appender를 실행한 뒤 상태 화면이 같은 오른쪽 FD 페이지를 이어받는
/// 원본 수명을 결속한다. 화면별 페이지를 고르지는 않으며, 중앙 상주 정책이
/// `publish -> retain` 전이를 채택할 수 있는 원천 경계만 제공한다.
pub(crate) fn bind_unit_summary_status_page_inheritance_source(source: &[u8]) -> Result<()> {
    let prg = source
        .get(HEADER_SIZE..HEADER_SIZE + PRG_SIZE)
        .context("supported source does not contain the complete PRG region")?;
    validate_code_regions(prg)?;

    let [unit_name_low, unit_name_high] = region("select_unit_name").cpu_address.to_le_bytes();
    let unit_name_call = [0x20, unit_name_low, unit_name_high];
    let summary = code_region_source(prg, "compose_unit_summary_header")?;
    ensure!(
        summary
            .windows(unit_name_call.len())
            .filter(|window| *window == unit_name_call)
            .count()
            == 1,
        "unit-summary composer no longer publishes exactly one unit-or-enemy name page"
    );
    let status = code_region_source(prg, "compose_unit_status_stats")?;
    ensure!(
        !status
            .windows(unit_name_call.len())
            .any(|window| window == unit_name_call),
        "unit-status composer unexpectedly gained an independent name-page publication"
    );
    Ok(())
}

/// 유닛 UI 도메인의 고정 라벨 population과 세 화면 소비자를 실제 원천 생산자에
/// 결속한다. 화면 목록은 전역 coverage 표에서 가져오지 않으며, 이 함수가 검증한
/// composer와 상속 경로만 반환한다.
pub(crate) fn inspect_unit_ui_translation_consumers(
    source: &[u8],
) -> Result<TranslationConsumerSourceEvidence> {
    let prg = source
        .get(HEADER_SIZE..HEADER_SIZE + PRG_SIZE)
        .context("supported source does not contain the complete PRG region")?;
    validate_code_regions(prg)?;
    let command_menu = command_menu::analyze(prg)?;
    validate_fixed_labels(
        prg,
        SUMMARY_AND_STATUS_LABEL_SPECS
            .iter()
            .chain(command_menu::COMMAND_LABEL_SPECS),
    )?;

    let mut population_ids = summary_and_status_label_ids();
    population_ids.extend(command_menu_label_ids());
    let summary_label_indices = summary_composer_fixed_label_indices(prg)?;
    let summary_population_ids =
        translated_summary_status_label_ids(summary_label_indices.iter().copied())?;
    let status_label_indices = status_composer_fixed_label_indices(prg)?;
    let status_population_ids = translated_summary_status_label_ids(
        status_label_indices
            .into_iter()
            .chain(summary_label_indices),
    )?;
    let command_population_ids = command_menu_label_ids();
    let dispatch = region("dispatch_composite_text_role_from_05e8");
    let summary_header = region("compose_unit_summary_header");
    let summary_items = region("compose_unit_summary_items");
    let status_stats = region("compose_unit_status_stats");
    Ok(TranslationConsumerSourceEvidence {
        population_ids,
        screen_bindings: vec![
            ScreenConsumerSourceBinding {
                screen_role: "unit_summary",
                population_ids: summary_population_ids,
                source_binding_ids: vec![
                    qualified_source_binding_id(
                        UNIT_UI_BANK,
                        dispatch.cpu_address,
                        dispatch.role,
                        "states=04,07",
                    ),
                    source_binding_id(
                        UNIT_UI_BANK,
                        summary_header.cpu_address,
                        summary_header.role,
                    ),
                    source_binding_id(UNIT_UI_BANK, summary_items.cpu_address, summary_items.role),
                ],
            },
            ScreenConsumerSourceBinding {
                screen_role: "unit_command_menu",
                population_ids: command_population_ids,
                source_binding_ids: vec![
                    qualified_source_binding_id(
                        UNIT_UI_BANK,
                        dispatch.cpu_address,
                        dispatch.role,
                        "state=05",
                    ),
                    source_binding_id(
                        command_menu.composer.prg_bank,
                        command_menu.composer.cpu_address,
                        command_menu.composer.role,
                    ),
                ],
            },
            ScreenConsumerSourceBinding {
                screen_role: "unit_status",
                population_ids: status_population_ids,
                source_binding_ids: vec![
                    qualified_source_binding_id(
                        UNIT_UI_BANK,
                        dispatch.cpu_address,
                        dispatch.role,
                        "state=0F",
                    ),
                    source_binding_id(UNIT_UI_BANK, status_stats.cpu_address, status_stats.role),
                    qualified_source_binding_id(
                        UNIT_UI_BANK,
                        summary_header.cpu_address,
                        summary_header.role,
                        "inherited_by=unit_status",
                    ),
                ],
            },
        ],
    })
}

fn summary_composer_fixed_label_indices(prg: &[u8]) -> Result<Vec<u8>> {
    let composer = code_region_source(prg, "compose_unit_summary_header")?;
    let indices = direct_fixed_label_indices(composer);
    ensure!(
        append_fixed_string_call_count(composer) == indices.len(),
        "unit-summary composer contains an unclassified fixed-label producer"
    );
    Ok(indices)
}

fn status_composer_fixed_label_indices(prg: &[u8]) -> Result<Vec<u8>> {
    let composer = code_region_source(prg, "compose_unit_status_stats")?;
    let direct_indices = direct_fixed_label_indices(composer);
    let append_call = append_fixed_string_call();
    let mut computed_ranges = composer.windows(15).filter_map(|window| {
        (window[0] == 0xA0
            && window[2..5] == [0x84, 0x12, 0xC0]
            && window[6] == 0xF0
            && window[8..11] == [0x98, 0x38, 0xE9]
            && window[12..15] == append_call)
            .then_some((window[1], window[5], window[11]))
    });
    let (start, end, subtrahend) = computed_ranges
        .next()
        .context("unit-status composer lost its computed fixed-label range")?;
    ensure!(
        computed_ranges.next().is_none(),
        "unit-status composer has multiple computed fixed-label ranges"
    );
    let computed_indices = (start..end)
        .map(|value| {
            value
                .checked_sub(subtrahend)
                .context("unit-status fixed-label range underflow")
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        append_fixed_string_call_count(composer) == direct_indices.len() + 1,
        "unit-status composer contains an unclassified fixed-label producer"
    );

    Ok(computed_indices.into_iter().chain(direct_indices).collect())
}

fn code_region_source<'a>(prg: &'a [u8], role: &str) -> Result<&'a [u8]> {
    let spec = region(role);
    let offset = banked_prg_offset(UNIT_UI_BANK, spec.cpu_address)?;
    prg.get(offset..offset + spec.expected.len())
        .with_context(|| format!("unit-UI code region {role} exceeds the source PRG"))
}

fn direct_fixed_label_indices(composer: &[u8]) -> Vec<u8> {
    let append_call = append_fixed_string_call();
    composer
        .windows(5)
        .filter_map(|window| {
            (window[0] == 0xA9 && window[2..5] == append_call).then_some(window[1])
        })
        .collect()
}

fn append_fixed_string_call_count(composer: &[u8]) -> usize {
    let append_call = append_fixed_string_call();
    composer
        .windows(append_call.len())
        .filter(|window| *window == append_call)
        .count()
}

fn append_fixed_string_call() -> [u8; 3] {
    let [low, high] = region("append_fixed_string").cpu_address.to_le_bytes();
    [0x20, low, high]
}

fn translated_summary_status_label_ids(
    indices: impl IntoIterator<Item = u8>,
) -> Result<Vec<String>> {
    let mut ids = BTreeSet::new();
    for index in indices {
        let spec = SUMMARY_AND_STATUS_LABEL_SPECS
            .iter()
            .find(|spec| spec.index == index)
            .with_context(|| format!("unknown unit-UI fixed label index 0x{index:02X}"))?;
        match spec.translation_scope {
            "japanese_only" => {
                ids.insert(format!("unit-ui-label:{index:02X}"));
            }
            "preserve_original_latin" => {}
            scope => {
                anyhow::bail!(
                    "unit-UI fixed label 0x{index:02X} has unknown translation scope {scope}"
                );
            }
        }
    }
    Ok(ids.into_iter().collect())
}

fn build_report(
    prg: &[u8],
    glyph_budget: glyph_budget::GlyphBudgetReport,
) -> Result<UnitUiTextReport> {
    ensure!(prg.len() == PRG_SIZE, "unexpected PRG size");
    validate_code_regions(prg)?;
    let command_menu = command_menu::analyze(prg)?;
    let fixed_labels = validate_fixed_labels(
        prg,
        SUMMARY_AND_STATUS_LABEL_SPECS
            .iter()
            .chain(command_menu::COMMAND_LABEL_SPECS),
    )?;
    ensure!(
        fixed_labels
            .iter()
            .filter(|label| label.translation_scope == "japanese_only")
            .count()
            == glyph_budget.fixed_japanese_label_count(),
        "unit-UI fixed-label count disagrees with the glyph budget"
    );

    Ok(UnitUiTextReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        screen_roles: vec![
            ScreenRoleReport {
                screen_role: "unit_summary",
                composers: vec!["unit_summary_header", "unit_summary_items"],
                inherited_content: vec![],
                translation_scope: "translate Japanese unit, class, level, and item text; preserve original Latin, digits, slash, and punctuation",
            },
            ScreenRoleReport {
                screen_role: "unit_command_menu",
                composers: vec!["unit_command_menu"],
                inherited_content: vec![],
                translation_scope: "translate all fifteen conditional Japanese command labels; do not infer one command set from the observed inventory-and-wait variant",
            },
            ScreenRoleReport {
                screen_role: "unit_status",
                composers: vec!["unit_status_stats"],
                inherited_content: vec!["unit_summary_header"],
                translation_scope: "translate nine Japanese stat labels; preserve inherited original Latin, digits, slash, and punctuation",
            },
        ],
        composition_dispatch: CompositionDispatchReport {
            state_address: "0x05E8",
            dispatcher: region_report(region("dispatch_composite_text_role_from_05e8")),
            relevant_states: vec![
                composition_state(0x04, "unit_summary_header", 0x826C),
                composition_state_with_report(
                    0x05,
                    "unit_command_menu",
                    command_menu.composer.clone(),
                ),
                composition_state(0x07, "unit_summary_items", 0x85BE),
                composition_state(0x0F, "unit_status_stats", 0x87F2),
            ],
        },
        shared_appenders: [
            "append_item_name_and_uses",
            "select_item_name",
            "select_unit_name",
            "select_class_name",
            "append_fixed_string",
            "append_number",
            "parse_composite_text",
        ]
        .into_iter()
        .map(|role| region_report(region(role)))
        .collect(),
        fixed_labels,
        dynamic_sources: vec![
            DynamicSourceReport {
                role: "unit_name",
                text_inventory_table_id: "unit-names or enemy-names",
                selected_by: "0B:8E88",
                destination: "composite buffer 0x0451,X",
            },
            DynamicSourceReport {
                role: "class_name",
                text_inventory_table_id: "class-names",
                selected_by: "0B:8EBA",
                destination: "composite buffer 0x0451,X",
            },
            DynamicSourceReport {
                role: "item_name",
                text_inventory_table_id: "item-names",
                selected_by: "0B:8E6B through 0B:875F",
                destination: "composite buffer 0x0451,X",
            },
        ],
        glyph_budget,
        command_menu,
        page_lifetime: PageLifetimeReport {
            right_fd_page_supplied_by_screen_roles: vec!["unit_summary", "unit_command_menu"],
            proven_inherited_by_screen_roles: vec!["unit_status"],
            phase_state_address: "0x05DE",
            phase_dispatcher: region_report(region("dispatch_unit_window_phase_from_05de")),
            right_fd_page_supply: region_report(region("open_unit_ui_right_fd_page_00")),
            runtime_evidence: "unit_summary and unit_command_menu entry each execute the right-FD supply; unit_summary-to-unit_status changes the left CHR pair without another right-FD supply",
            runtime_variant_coverage: "unit_summary, unit_command_menu, and unit_status each have runtime right 00/15, 00/18, and 00/19 evidence",
        },
        implementation_boundary: ImplementationBoundary {
            required_design: "budget one unit-UI family across four composition roles, fifteen command labels, and shared dynamic source tables while preserving every observed backing-page variant",
            rejected_shortcut: "per-unit or per-visible-string byte patches",
            preserved_original: ["Latin letters", "digits", "punctuation and slash"],
            separate_screen_contract: "automatic class_profile",
        },
    })
}

fn validate_code_regions(prg: &[u8]) -> Result<()> {
    for spec in CODE_REGION_SPECS {
        let offset = banked_prg_offset(UNIT_UI_BANK, spec.cpu_address)?;
        let end = offset + spec.expected.len();
        ensure!(
            end <= prg.len() && &prg[offset..end] == spec.expected,
            "unit-UI code contract mismatch for {} at bank 0B:{:04X}",
            spec.role,
            spec.cpu_address
        );
    }
    Ok(())
}

fn validate_fixed_labels<'a>(
    prg: &[u8],
    specs: impl IntoIterator<Item = &'a FixedLabelSpec>,
) -> Result<Vec<FixedLabelReport>> {
    specs
        .into_iter()
        .map(|spec| {
            let pointer_address = FIXED_STRING_POINTER_TABLE_ADDRESS + u16::from(spec.index) * 2;
            let pointer_offset = banked_prg_offset(UNIT_UI_BANK, pointer_address)?;
            let actual_pointer = u16::from_le_bytes([prg[pointer_offset], prg[pointer_offset + 1]]);
            ensure!(
                actual_pointer == spec.pointer,
                "fixed-label pointer mismatch for index 0x{:02X}: expected 0x{:04X}, found 0x{:04X}",
                spec.index,
                spec.pointer,
                actual_pointer
            );

            let source_offset = banked_prg_offset(UNIT_UI_BANK, spec.pointer)?;
            let source_end = source_offset + spec.expected.len();
            ensure!(
                source_end <= prg.len() && &prg[source_offset..source_end] == spec.expected,
                "fixed-label bytes mismatch for index 0x{:02X} at bank 0B:{:04X}",
                spec.index,
                spec.pointer
            );
            let terminator = *spec
                .expected
                .last()
                .context("fixed-label source bytes are empty")?;
            let source_display_cell_count =
                terminated_composite_display_cell_count(spec.expected, terminator)?;

            Ok(FixedLabelReport {
                index: spec.index,
                index_hex: format!("0x{:02X}", spec.index),
                source_text: spec.source_text,
                translation_scope: spec.translation_scope,
                pointer_table_address: pointer_address,
                pointer_table_address_hex: format!("0x{pointer_address:04X}"),
                source_address: spec.pointer,
                source_address_hex: format!("0x{:04X}", spec.pointer),
                source_storage_byte_count: spec.expected.len(),
                source_display_cell_count,
                source_codes: spec.expected.to_vec(),
                source_codes_hex: spec
                    .expected
                    .iter()
                    .map(|code| format!("0x{code:02X}"))
                    .collect(),
            })
        })
        .collect()
}

fn composition_state(
    state: u8,
    role: &'static str,
    handler_address: u16,
) -> CompositionStateReport {
    let handler = region(match role {
        "unit_summary_header" => "compose_unit_summary_header",
        "unit_summary_items" => "compose_unit_summary_items",
        "unit_status_stats" => "compose_unit_status_stats",
        _ => unreachable!("unknown unit-UI composition role"),
    });
    assert_eq!(handler.cpu_address, handler_address);
    CompositionStateReport {
        state,
        state_hex: format!("0x{state:02X}"),
        role,
        handler_address,
        handler_address_hex: format!("0x{handler_address:04X}"),
        handler: region_report(handler),
    }
}

fn composition_state_with_report(
    state: u8,
    role: &'static str,
    handler: CodeRegionReport,
) -> CompositionStateReport {
    CompositionStateReport {
        state,
        state_hex: format!("0x{state:02X}"),
        role,
        handler_address: handler.cpu_address,
        handler_address_hex: handler.cpu_address_hex.clone(),
        handler,
    }
}

fn region(role: &str) -> &'static CodeRegionSpec {
    CODE_REGION_SPECS
        .iter()
        .find(|spec| spec.role == role)
        .unwrap_or_else(|| panic!("missing code-region spec for {role}"))
}

fn region_report(spec: &CodeRegionSpec) -> CodeRegionReport {
    let prg_offset = banked_prg_offset(UNIT_UI_BANK, spec.cpu_address)
        .expect("static unit-UI region address must be valid");
    let file_offset = HEADER_SIZE + prg_offset;
    CodeRegionReport {
        role: spec.role,
        prg_bank: UNIT_UI_BANK,
        prg_bank_hex: format!("0x{UNIT_UI_BANK:02X}"),
        cpu_address: spec.cpu_address,
        cpu_address_hex: format!("0x{:04X}", spec.cpu_address),
        file_offset,
        file_offset_hex: format!("0x{file_offset:05X}"),
        byte_count: spec.expected.len(),
        bytes_hex: hex(spec.expected),
    }
}

pub(super) fn banked_prg_offset(bank: usize, cpu_address: u16) -> Result<usize> {
    ensure!(bank < PRG_SIZE / PRG_BANK_SIZE, "PRG bank out of range");
    ensure!(
        (SWITCHABLE_CPU_BASE..0xC000).contains(&cpu_address),
        "banked CPU address must be in 0x8000..0xBFFF"
    );
    Ok(bank * PRG_BANK_SIZE + usize::from(cpu_address - SWITCHABLE_CPU_BASE))
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}
