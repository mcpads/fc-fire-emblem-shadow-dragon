use anyhow::{Result, ensure};
use serde::Serialize;

use super::{CodeRegionReport, FixedLabelSpec, UNIT_UI_BANK, banked_prg_offset, fixed_label, hex};
use crate::rom::HEADER_SIZE;

const COMPOSER_ADDRESS: u16 = 0x82E3;
const COMPOSER_HEX: &str = concat!(
    "A90A8DCF0520C897A9308500A9778501A9408502A90085032025C2203C8E8A48A9008544A90620FAC968AA18AD3177F006A90E20EE8E386EEB05",
    "207F849016A592300CAD5177D00718A5922920F006A91020EE8E386EEB05A001B174C910F0031890198A48A9058544A90620FAC968AA18AD5177F006",
    "A93C20EE8E386EEB05A000B174C901D021A002AD7E76F001C88412A412B95D84206584B006C61210F23007A412B96184D01CA001B174C909D00BA9AB",
    "2065849004A937D009A9462065849006A92A20EE8E386EEB05A000841218A412B9B5EDF046A000D174F004E612D0EEA412B9C4ED30052011F1D00320",
    "1AF1B9C4ED209EF0B0E5A010B10038ED010520B8C58502A011B10038ED000520B8C5186502C902B0C6A93B20EE8E386EEB05A013B174D008C8C01790",
    "F7189006A90F20EE8E386EEB0518ADD077F008A8B9578420EE8E386EEB05A91120EE8E386EEB05A9EF9D5104ADEB052040980A1869028DD005AECE05AD",
    "EB059DEE7FA9019DF37F4C398F",
);
const SELECTION_TABLES_ADDRESS: u16 = 0x8457;
const SELECTION_TABLES: [u8; 14] = [
    0x00, 0x29, 0x33, 0x34, 0x3A, 0x3D, // facility index -> label index
    0xA5, 0x4B, 0xAE, 0xAB, // map-tile candidates
    0x2A, 0x38, 0x39, 0x37, // matching terrain-action labels
];
const TERRAIN_PREDICATE_ADDRESS: u16 = 0x8465;
const TERRAIN_PREDICATE: [u8; 26] = [
    0x48, 0xAD, 0x01, 0x05, 0x0A, 0xA8, 0xB9, 0x3D, 0xED, 0x85, 0x00, 0xB9, 0x3E, 0xED, 0x85, 0x01,
    0xAC, 0x00, 0x05, 0x68, 0xD1, 0x00, 0xF0, 0x01, 0x18, 0x60,
];

pub(super) const COMMAND_LABEL_SPECS: &[FixedLabelSpec] = &[
    fixed_label(
        0x0E,
        "こうげき",
        "japanese_only",
        0x90BF,
        &[0x09, 0x02, 0x08, 0x0F, 0x06, 0xED],
    ),
    fixed_label(
        0x0F,
        "もちもの",
        "japanese_only",
        0x90C5,
        &[0x24, 0x11, 0x24, 0x19, 0xED],
    ),
    fixed_label(0x10, "つえ", "japanese_only", 0x90CA, &[0x12, 0x03, 0xED]),
    fixed_label(
        0x11,
        "たいき",
        "japanese_only",
        0x90CD,
        &[0x10, 0x01, 0x06, 0xED],
    ),
    fixed_label(
        0x29,
        "ぶきや",
        "japanese_only",
        0x9170,
        &[0x1C, 0x0F, 0x06, 0x25, 0xED],
    ),
    fixed_label(
        0x2A,
        "たずねる",
        "japanese_only",
        0x9175,
        &[0x10, 0x0C, 0x0F, 0x18, 0x2A, 0xED],
    ),
    fixed_label(
        0x33,
        "どうぐや",
        "japanese_only",
        0x91B5,
        &[0x14, 0x0F, 0x02, 0x07, 0x0F, 0x25, 0xED],
    ),
    fixed_label(
        0x34,
        "とうぎじょう",
        "japanese_only",
        0x91BC,
        &[0x14, 0x02, 0x06, 0x0F, 0x0B, 0x0F, 0x87, 0x02, 0xED],
    ),
    fixed_label(
        0x37,
        "たからばこ",
        "japanese_only",
        0x91D1,
        &[0x10, 0x05, 0x28, 0x1A, 0x0F, 0x09, 0xED],
    ),
    fixed_label(0x38, "しろ", "japanese_only", 0x91D8, &[0x0B, 0x2C, 0xED]),
    fixed_label(
        0x39,
        "ぎょくざ",
        "japanese_only",
        0x91DB,
        &[0x06, 0x0F, 0x87, 0x07, 0x0A, 0x0F, 0xED],
    ),
    fixed_label(
        0x3A,
        "あずかりじょ",
        "japanese_only",
        0x91E2,
        &[0x00, 0x0C, 0x0F, 0x05, 0x29, 0x0B, 0x0F, 0x87, 0xED],
    ),
    fixed_label(
        0x3B,
        "はなす",
        "japanese_only",
        0x91EB,
        &[0x1A, 0x15, 0x0C, 0xED],
    ),
    fixed_label(
        0x3C,
        "へんしん",
        "japanese_only",
        0x91EF,
        &[0x1D, 0x2F, 0x0B, 0x2F, 0xED],
    ),
    fixed_label(
        0x3D,
        "ひみつのみせ",
        "japanese_only",
        0x91F4,
        &[0x1B, 0x21, 0x12, 0x19, 0x21, 0x0D, 0xED],
    ),
];

#[derive(Debug, Serialize)]
pub(super) struct CommandMenuReport {
    screen_role: &'static str,
    composer_state: u8,
    composer_state_hex: &'static str,
    pub(super) composer: CodeRegionReport,
    fixed_label_indices: Vec<u8>,
    fixed_label_indices_hex: Vec<String>,
    selection_groups: Vec<SelectionGroup>,
    decision_flow: Vec<DecisionStepReport>,
    terrain_actions: Vec<TerrainActionReport>,
    facility_actions: Vec<FacilityActionReport>,
    input_boundary: InputBoundaryReport,
    runtime_observed_label_indices: [u8; 2],
    runtime_observed_label_indices_hex: [&'static str; 2],
    pub(super) runtime_observed_label_count: usize,
    pub(super) static_label_count: usize,
    page_lifetime_boundary: &'static str,
    next_gate: &'static str,
}

#[derive(Debug, Serialize)]
struct SelectionGroup {
    role: &'static str,
    source_signal: &'static str,
    label_indices_hex: Vec<String>,
}

#[derive(Debug, Serialize)]
struct DecisionStepReport {
    selection_mask_bit: u8,
    role: &'static str,
    label_indices_hex: Vec<String>,
    source_labels: Vec<&'static str>,
    inclusion_condition: &'static str,
}

#[derive(Debug, Serialize)]
struct TerrainActionReport {
    map_tile_code: u8,
    map_tile_code_hex: String,
    label_index: u8,
    label_index_hex: String,
    source_label: &'static str,
    eligibility: &'static str,
}

#[derive(Debug, Serialize)]
struct FacilityActionReport {
    facility_index: u8,
    label_index: u8,
    label_index_hex: String,
    source_label: &'static str,
}

#[derive(Debug, Serialize)]
struct InputBoundaryReport {
    state_kind: &'static str,
    confirmed_menu_entry: &'static str,
    confirmed_safe_exit: &'static str,
    unproven_selection_action: &'static str,
    observation_rule: &'static str,
    idle_behavior: &'static str,
}

pub(super) fn analyze(prg: &[u8]) -> Result<CommandMenuReport> {
    let expected = decode_composer_hex()?;
    let composer_offset = banked_prg_offset(UNIT_UI_BANK, COMPOSER_ADDRESS)?;
    let composer_end = composer_offset + expected.len();
    ensure!(
        composer_end <= prg.len() && prg[composer_offset..composer_end] == expected,
        "unit-command-menu composer mismatch at bank 0B:82E3"
    );
    validate_region(
        prg,
        SELECTION_TABLES_ADDRESS,
        &SELECTION_TABLES,
        "unit-command-menu selection tables",
    )?;
    validate_region(
        prg,
        TERRAIN_PREDICATE_ADDRESS,
        &TERRAIN_PREDICATE,
        "unit-command-menu terrain predicate",
    )?;

    let file_offset = HEADER_SIZE + composer_offset;
    let composer = CodeRegionReport {
        role: "compose_unit_command_menu",
        prg_bank: UNIT_UI_BANK,
        prg_bank_hex: format!("0x{UNIT_UI_BANK:02X}"),
        cpu_address: COMPOSER_ADDRESS,
        cpu_address_hex: format!("0x{COMPOSER_ADDRESS:04X}"),
        file_offset,
        file_offset_hex: format!("0x{file_offset:05X}"),
        byte_count: expected.len(),
        bytes_hex: hex(&expected),
    };
    let fixed_label_indices = COMMAND_LABEL_SPECS
        .iter()
        .map(|label| label.index)
        .collect::<Vec<_>>();

    Ok(CommandMenuReport {
        screen_role: "unit_command_menu",
        composer_state: 0x05,
        composer_state_hex: "0x05",
        composer,
        fixed_label_indices_hex: fixed_label_indices
            .iter()
            .map(|index| format!("0x{index:02X}"))
            .collect(),
        fixed_label_indices,
        selection_groups: vec![
            selection_group(
                "direct unit capability",
                "0x7731, 0x7751, unit class, and helper 0x847F",
                &[0x0E, 0x10, 0x3C],
            ),
            selection_group(
                "terrain action",
                "map-tile candidates 0xA5/0x4B/0xAE/0xAB through 0x8465",
                &[0x2A, 0x38, 0x39, 0x37],
            ),
            selection_group(
                "adjacent talk target",
                "unit tables at 0xEDB5/0xEDC4 plus distance checks",
                &[0x3B],
            ),
            selection_group(
                "nonempty inventory",
                "unit record offsets 0x13 through 0x16",
                &[0x0F],
            ),
            selection_group(
                "map facility",
                "0x77D0 indexes the table at 0x8457",
                &[0x29, 0x33, 0x34, 0x3A, 0x3D],
            ),
            selection_group(
                "unconditional final action",
                "literal append before composite parse",
                &[0x11],
            ),
        ],
        decision_flow: vec![
            decision_step(0, "attack", &[0x0E], "0x7731 is nonzero"),
            decision_step(
                1,
                "staff",
                &[0x10],
                "helper 0B:847F returns carry set and either 0x0092 bit 7, 0x7751 nonzero, or 0x0092 bit 5 is set",
            ),
            decision_step(
                2,
                "transform",
                &[0x3C],
                "unit-record class byte at offset 0x01 equals 0x10 and the bank-06 helper leaves 0x7751 nonzero",
            ),
            decision_step(
                3,
                "terrain_action",
                &[0x2A, 0x38, 0x39, 0x37],
                "the current map tile matches an eligible terrain candidate through 0B:8465",
            ),
            decision_step(
                4,
                "talk",
                &[0x3B],
                "the unit tables at 0xEDB5/0xEDC4 resolve an eligible target and the absolute X/Y distance sum is below 2",
            ),
            decision_step(
                5,
                "inventory",
                &[0x0F],
                "any unit-record item slot at offsets 0x13 through 0x16 is nonzero",
            ),
            decision_step(
                6,
                "facility",
                &[0x29, 0x33, 0x34, 0x3A, 0x3D],
                "0x77D0 is a nonzero facility index into 0B:8457",
            ),
            decision_step(7, "wait", &[0x11], "unconditional final command"),
        ],
        terrain_actions: vec![
            terrain_action(0xA5, 0x2A, "unit id 0x01"),
            terrain_action(0x4B, 0x38, "unit id 0x01"),
            terrain_action(0xAE, 0x39, "unit id 0x01"),
            terrain_action(
                0xAB,
                0x37,
                "unit id 0x01 when 0x767E is nonzero, or class id 0x09",
            ),
            terrain_action(0x46, 0x2A, "any unit through the separate fallback"),
        ],
        facility_actions: (1_u8..=5)
            .map(|facility_index| {
                facility_action(
                    facility_index,
                    SELECTION_TABLES[usize::from(facility_index)],
                )
            })
            .collect(),
        input_boundary: InputBoundaryReport {
            state_kind: "input_waiting_command_menu",
            confirmed_menu_entry: "A on the unit's current tile after movement selection opens the command menu without relocating the unit",
            confirmed_safe_exit: "B returns from the command menu to unit_summary without executing a listed command",
            unproven_selection_action: "A inside the command menu executes the highlighted command; individual action handlers are not yet bound by this report",
            observation_rule: "use entry plus B exit only; do not press A inside the menu until the highlighted label and its action handler are both known",
            idle_behavior: "152 input-free frames kept the CHR shadows fixed while only cursor and map-sprite animation phases changed",
        },
        runtime_observed_label_indices: [0x0F, 0x11],
        runtime_observed_label_indices_hex: ["0x0F", "0x11"],
        runtime_observed_label_count: 2,
        static_label_count: COMMAND_LABEL_SPECS.len(),
        page_lifetime_boundary: "command-menu entry executes the central right-FD supply at 0xC9C2 with composite state 0x05; one observed entry supplies 00 while the backing FE page is 15",
        next_gate: "observe representative terrain and facility variants, determine whether a right 00/19 variant is reachable, and include all fifteen labels in the unit-UI glyph union",
    })
}

fn selection_group(
    role: &'static str,
    source_signal: &'static str,
    label_indices: &[u8],
) -> SelectionGroup {
    SelectionGroup {
        role,
        source_signal,
        label_indices_hex: label_indices
            .iter()
            .map(|index| format!("0x{index:02X}"))
            .collect(),
    }
}

fn decision_step(
    selection_mask_bit: u8,
    role: &'static str,
    label_indices: &[u8],
    inclusion_condition: &'static str,
) -> DecisionStepReport {
    DecisionStepReport {
        selection_mask_bit,
        role,
        label_indices_hex: label_indices
            .iter()
            .map(|index| format!("0x{index:02X}"))
            .collect(),
        source_labels: label_indices
            .iter()
            .map(|index| command_label(*index).source_text)
            .collect(),
        inclusion_condition,
    }
}

fn terrain_action(
    map_tile_code: u8,
    label_index: u8,
    eligibility: &'static str,
) -> TerrainActionReport {
    TerrainActionReport {
        map_tile_code,
        map_tile_code_hex: format!("0x{map_tile_code:02X}"),
        label_index,
        label_index_hex: format!("0x{label_index:02X}"),
        source_label: command_label(label_index).source_text,
        eligibility,
    }
}

fn facility_action(facility_index: u8, label_index: u8) -> FacilityActionReport {
    FacilityActionReport {
        facility_index,
        label_index,
        label_index_hex: format!("0x{label_index:02X}"),
        source_label: command_label(label_index).source_text,
    }
}

fn command_label(index: u8) -> &'static FixedLabelSpec {
    COMMAND_LABEL_SPECS
        .iter()
        .find(|label| label.index == index)
        .unwrap_or_else(|| panic!("missing command label 0x{index:02X}"))
}

fn validate_region(prg: &[u8], address: u16, expected: &[u8], role: &str) -> Result<()> {
    let offset = banked_prg_offset(UNIT_UI_BANK, address)?;
    let end = offset + expected.len();
    ensure!(
        end <= prg.len() && &prg[offset..end] == expected,
        "{role} mismatch at bank 0B:{address:04X}"
    );
    Ok(())
}

fn decode_composer_hex() -> Result<Vec<u8>> {
    ensure!(
        COMPOSER_HEX.len().is_multiple_of(2),
        "unit-command-menu composer hex has an odd length"
    );
    (0..COMPOSER_HEX.len())
        .step_by(2)
        .map(|offset| {
            u8::from_str_radix(&COMPOSER_HEX[offset..offset + 2], 16)
                .map_err(|error| anyhow::anyhow!("invalid command-menu composer hex: {error}"))
        })
        .collect()
}

#[cfg(test)]
pub(super) fn install_fixture(prg: &mut [u8]) {
    let composer = decode_composer_hex().unwrap();
    let composer_offset = banked_prg_offset(UNIT_UI_BANK, COMPOSER_ADDRESS).unwrap();
    prg[composer_offset..composer_offset + composer.len()].copy_from_slice(&composer);
    let tables_offset = banked_prg_offset(UNIT_UI_BANK, SELECTION_TABLES_ADDRESS).unwrap();
    prg[tables_offset..tables_offset + SELECTION_TABLES.len()].copy_from_slice(&SELECTION_TABLES);
    let predicate_offset = banked_prg_offset(UNIT_UI_BANK, TERRAIN_PREDICATE_ADDRESS).unwrap();
    prg[predicate_offset..predicate_offset + TERRAIN_PREDICATE.len()]
        .copy_from_slice(&TERRAIN_PREDICATE);
}

#[cfg(test)]
pub(super) fn composer_address() -> u16 {
    COMPOSER_ADDRESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::PRG_SIZE;

    fn report() -> CommandMenuReport {
        let mut prg = vec![0; PRG_SIZE];
        install_fixture(&mut prg);
        analyze(&prg).unwrap()
    }

    #[test]
    fn binds_selection_mask_bits_to_decision_order() {
        let report = report();
        assert_eq!(
            report
                .decision_flow
                .iter()
                .map(|step| (step.selection_mask_bit, step.role))
                .collect::<Vec<_>>(),
            [
                (0, "attack"),
                (1, "staff"),
                (2, "transform"),
                (3, "terrain_action"),
                (4, "talk"),
                (5, "inventory"),
                (6, "facility"),
                (7, "wait"),
            ]
        );
    }

    #[test]
    fn resolves_terrain_and_facility_codes_to_source_labels() {
        let report = report();
        assert_eq!(
            report
                .terrain_actions
                .iter()
                .map(|action| (
                    action.map_tile_code,
                    action.label_index,
                    action.source_label
                ))
                .collect::<Vec<_>>(),
            [
                (0xA5, 0x2A, "たずねる"),
                (0x4B, 0x38, "しろ"),
                (0xAE, 0x39, "ぎょくざ"),
                (0xAB, 0x37, "たからばこ"),
                (0x46, 0x2A, "たずねる"),
            ]
        );
        assert_eq!(
            report
                .facility_actions
                .iter()
                .map(|action| (
                    action.facility_index,
                    action.label_index,
                    action.source_label
                ))
                .collect::<Vec<_>>(),
            [
                (1, 0x29, "ぶきや"),
                (2, 0x33, "どうぐや"),
                (3, 0x34, "とうぎじょう"),
                (4, 0x3A, "あずかりじょ"),
                (5, 0x3D, "ひみつのみせ"),
            ]
        );
    }

    #[test]
    fn keeps_unbound_command_execution_out_of_the_observation_path() {
        let report = report();
        assert_eq!(
            report.input_boundary.state_kind,
            "input_waiting_command_menu"
        );
        assert!(report.input_boundary.confirmed_safe_exit.starts_with('B'));
        assert!(
            report
                .input_boundary
                .unproven_selection_action
                .starts_with('A')
        );
        assert!(
            report
                .input_boundary
                .observation_rule
                .contains("do not press A")
        );
    }
}
