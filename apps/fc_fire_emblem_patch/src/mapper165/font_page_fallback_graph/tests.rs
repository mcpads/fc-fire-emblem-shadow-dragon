use std::collections::BTreeSet;

use super::super::{
    battle_composition_runtime::CUMULATIVE_RUNTIME_LAYOUT,
    maximum_dialogue_runtime::build_initial_page_selector,
    options_page::{ROW_OWNER_GATE_ADDRESS, build_row_owner_gate},
    roster_page::{
        CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS, CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS,
    },
};
use super::*;
use crate::rp2a03::{Instruction, assemble_at};

const SYNTHETIC_CHR_BANK_COUNT: u8 = 32;
const MAXIMUM_INITIAL_POINTER: u16 = 0x8FF1;

fn installed_candidate() -> Rom {
    let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
    bytes[5] = SYNTHETIC_CHR_BANK_COUNT;
    bytes.resize(
        bytes.len() + usize::from(SYNTHETIC_CHR_BANK_COUNT) * 8 * 1024,
        0,
    );
    let central =
        cumulative_battle_central_right_fd_selector(INITIAL_PAGE_SELECTOR_ADDRESS).unwrap();
    install_fixed(
        &mut bytes,
        CUMULATIVE_RUNTIME_LAYOUT.battle_central_right_fd_selector,
        &central,
    );
    let maximum =
        build_initial_page_selector(ROSTER_SELECTOR_ADDRESS, MAXIMUM_INITIAL_POINTER).unwrap();
    install_fixed(&mut bytes, INITIAL_PAGE_SELECTOR_ADDRESS, &maximum);
    let options = build_options_selector(
        OPTIONS_PAGE_A_REGISTER,
        OPTIONS_PAGE_B_REGISTER,
        ROSTER_SELECTOR_ADDRESS,
    )
    .unwrap();
    install_fixed(&mut bytes, OPTIONS_SELECTOR_ADDRESS, &options);
    install_fixed(
        &mut bytes,
        ROW_OWNER_GATE_ADDRESS,
        &build_row_owner_gate().unwrap(),
    );
    let roster = build_roster_selector(
        ROSTER_PAGE_REGISTERS[0],
        ROSTER_PAGE_REGISTERS[1],
        UNIT_SELECTOR_ADDRESS,
    )
    .unwrap();
    install_fixed(&mut bytes, ROSTER_SELECTOR_ADDRESS, &roster);
    let unit =
        super::super::unit_name_page::build_page_selector(0xB0, SHOP_SELECTOR_ADDRESS).unwrap();
    install_fixed(&mut bytes, UNIT_SELECTOR_ADDRESS, &unit);
    let shop = build_shop_selector(0xC0, FRONT_END_SELECTOR_ADDRESS).unwrap();
    install_fixed(&mut bytes, SHOP_SELECTOR_ADDRESS, &shop);
    let front = super::super::front_end_page::build_page_selector(
        0xA8,
        DIALOGUE_FONT_PAGE_SELECTOR_ADDRESS,
    )
    .unwrap();
    install_fixed(&mut bytes, FRONT_END_SELECTOR_ADDRESS, &front);
    let dialogue = build_chapter_page_selector(
        DIALOGUE_FONT_PAGE_SELECTOR_ADDRESS,
        ChapterPageSequence {
            admitted_chapter_count: CUMULATIVE_DIALOGUE_CHAPTER_COUNT,
            first_mapper_register: 0x98,
        },
        SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS,
    )
    .unwrap();
    install_fixed(&mut bytes, DIALOGUE_FONT_PAGE_SELECTOR_ADDRESS, &dialogue);
    install_fixed(
        &mut bytes,
        CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS,
        &assemble_at(
            CENTRAL_RIGHT_FD_SELECTOR_CALL_ADDRESS,
            &[Instruction::JsrAbsolute(
                CUMULATIVE_RUNTIME_LAYOUT.battle_central_right_fd_selector,
            )],
        )
        .unwrap(),
    );
    install_fixed(
        &mut bytes,
        CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS,
        &assemble_at(
            CENTRAL_RIGHT_FE_COMPANION_FD_REFRESH_CALL_ADDRESS,
            &[Instruction::JsrAbsolute(
                CUMULATIVE_RUNTIME_LAYOUT.battle_central_right_fd_selector,
            )],
        )
        .unwrap(),
    );
    Rom::parse(bytes).unwrap()
}

fn install_fixed(bytes: &mut [u8], address: u16, replacement: &[u8]) {
    let offset = crate::test_support::synthetic_fixed_bank_file_offset(address);
    bytes[offset..offset + replacement.len()].copy_from_slice(replacement);
}

#[test]
fn binds_the_branching_cumulative_fallback_graph() {
    let graph = bind_cumulative_font_page_fallback_graph(&installed_candidate()).unwrap();

    assert_eq!(graph.nodes.len(), 8);
    assert_eq!(graph.routes.len(), 11);
    assert_eq!(graph.direct_entry_candidate_count, 9);
    assert_eq!(graph.conditional_entry_count, 1);
    assert_eq!(graph.terminal_fallback_count, 1);
    assert_eq!(graph.unit_name_selector().mapper_register, 0xB0);
    assert_eq!(graph.front_end_selector().mapper_register, 0xA8);
    assert_eq!(
        graph
            .routes
            .iter()
            .filter(|route| route.target_role == FontPageFallbackNodeRole::UnitRoster.id())
            .map(|route| route.source_role)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            FontPageFallbackNodeRole::MaximumDialogue.id(),
            FontPageFallbackNodeRole::OptionsMenu.id(),
        ])
    );
}

#[test]
fn rejects_an_unclassified_direct_entry_into_any_node() {
    let mut bytes = installed_candidate().data().to_vec();
    install_fixed(
        &mut bytes,
        0xC100,
        &assemble_at(0xC100, &[Instruction::JmpAbsolute(SHOP_SELECTOR_ADDRESS)]).unwrap(),
    );

    let error = bind_cumulative_font_page_fallback_graph(&Rom::parse(bytes).unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("direct-entry candidate census changed"));
}

#[test]
fn rejects_a_drifted_options_branch_before_reclassifying_the_graph() {
    let mut bytes = installed_candidate().data().to_vec();
    let offset = crate::test_support::synthetic_fixed_bank_file_offset(ROW_OWNER_GATE_ADDRESS);
    bytes[offset + 6] ^= 1;

    let error = bind_cumulative_font_page_fallback_graph(&Rom::parse(bytes).unwrap())
        .unwrap_err()
        .to_string();
    assert!(error.contains("options row-owner gate"));
}
