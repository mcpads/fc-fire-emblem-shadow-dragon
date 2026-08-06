use crate::rp2a03::{Instruction, assemble_at};

use super::*;

#[test]
fn routines_fit_disjoint_ranges_inside_the_proven_cave() {
    let routines = build_routines(&[]).unwrap();
    validate_routine_placements(&routines).unwrap();

    assert_eq!(routines.len(), 8);
}

#[test]
fn prg_selector_maps_one_mmc4_bank_to_two_consecutive_mmc3_banks() {
    let bytes = &build_routines(&[]).unwrap()[1].bytes;
    assert!(bytes.windows(3).any(|window| window == [0x8D, 0x00, 0x80]));
    assert!(bytes.windows(3).any(|window| window == [0x8D, 0x01, 0x80]));
    assert!(bytes.windows(2).any(|window| window == [0x29, 0x0F]));
    assert!(bytes.windows(2).any(|window| window == [0x09, 0x01]));
}

#[test]
fn chr_selectors_bias_source_pages_away_from_chr_ram() {
    for routine in build_routines(&[]).unwrap().iter().skip(2).take(4) {
        assert!(
            routine
                .bytes
                .windows(2)
                .any(|window| window == [0x29, 0x1F])
        );
        assert!(
            routine
                .bytes
                .windows(2)
                .any(|window| window == [0x69, 0x08])
        );
    }

    assert_eq!(map_source_chr_page(0), 8);
    assert_eq!(map_source_chr_page(31), 132);
    assert_eq!(map_source_chr_page(0xFF), 132);
}

#[test]
fn central_writer_redirects_preserve_source_lengths() {
    let source_prg = assemble_at(
        SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS,
        &[
            Instruction::StaZeroPage(0x29),
            Instruction::StaZeroPage(0x51),
            Instruction::StaAbsolute(0xA000),
            Instruction::Rts,
        ],
    )
    .unwrap();
    let target_prg = assemble_at(
        SOURCE_SELECT_PRG_BANK_AND_SAVE_ADDRESS,
        &[
            Instruction::StaZeroPage(0x29),
            Instruction::StaZeroPage(0x51),
            Instruction::JsrAbsolute(SELECT_PRG_BANK_ADDRESS),
            Instruction::Rts,
        ],
    )
    .unwrap();
    assert_eq!(source_prg.len(), target_prg.len());

    for writer in CENTRAL_CHR_WRITERS {
        let source = assemble_at(
            writer.source_address,
            &[
                Instruction::StaZeroPage(writer.shadow_address),
                Instruction::OraZeroPage(0x52),
                Instruction::StaAbsolute(writer.source_register),
                Instruction::Rts,
            ],
        )
        .unwrap();
        let replacement = assemble_at(
            writer.source_address,
            &[
                Instruction::StaZeroPage(writer.shadow_address),
                Instruction::OraZeroPage(0x52),
                Instruction::JsrAbsolute(writer.target_routine),
                Instruction::Rts,
            ],
        )
        .unwrap();
        assert_eq!(source.len(), replacement.len());
    }
}

#[test]
fn direct_writer_redirects_keep_three_byte_instruction_size() {
    for writer in SOURCE_PRG_BANK_WRITERS.iter().chain(DIRECT_CHR_WRITERS) {
        let source = assemble_at(
            writer.source_address,
            &[Instruction::StaAbsolute(writer.source_register)],
        )
        .unwrap();
        let replacement = assemble_at(
            writer.source_address,
            &[Instruction::JsrAbsolute(writer.target_routine)],
        )
        .unwrap();
        assert_eq!(source.len(), 3);
        assert_eq!(replacement.len(), source.len());
    }
}

#[test]
fn pair_aware_right_selector_preserves_a_and_flags_and_selects_the_variant() {
    let entry = trigger_variants::PairSelectorEntry {
        pattern_window: trigger_planes::PatternWindow::Right,
        fd_source_page: 0,
        fe_source_page: 0x14,
        mapper_register_value: 4,
    };
    let routines = build_routines(&[entry]).unwrap();
    let selector = routines
        .iter()
        .find(|routine| routine.cpu_address == SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS)
        .unwrap();

    assert_eq!(&selector.bytes[..2], &[0x08, 0x48]);
    assert_eq!(
        &selector.bytes[selector.bytes.len() - 3..],
        &[0x68, 0x28, 0x60]
    );
    assert!(
        selector
            .bytes
            .windows(8)
            .any(|bytes| bytes == [0xA5, 0x5B, 0x05, 0x52, 0x29, 0x1F, 0xC9, 0x00])
    );
    assert!(
        selector
            .bytes
            .windows(8)
            .any(|bytes| bytes == [0xA5, 0x5C, 0x05, 0x52, 0x29, 0x1F, 0xC9, 0x14])
    );
    assert!(selector.bytes.windows(2).any(|bytes| bytes == [0xA9, 0x04]));
}

#[test]
fn central_fe_refreshes_pair_selection_while_direct_writers_stay_stateless() {
    let central_right_fe = CENTRAL_CHR_WRITERS
        .iter()
        .find(|writer| writer.source_register == 0xE000)
        .unwrap();
    assert_eq!(
        central_right_fe.target_routine,
        SELECT_CENTRAL_RIGHT_FE_CHR_BANK_ADDRESS
    );
    assert!(
        DIRECT_CHR_WRITERS
            .iter()
            .filter(|writer| writer.source_register == 0xE000)
            .all(|writer| writer.target_routine == SELECT_RIGHT_FE_CHR_BANK_ADDRESS)
    );

    let wrapper = build_routines(&[])
        .unwrap()
        .into_iter()
        .find(|routine| routine.cpu_address == SELECT_CENTRAL_RIGHT_FE_CHR_BANK_ADDRESS)
        .unwrap();
    let expected = assemble_at(
        SELECT_CENTRAL_RIGHT_FE_CHR_BANK_ADDRESS,
        &[
            Instruction::JsrAbsolute(SELECT_RIGHT_FE_CHR_BANK_ADDRESS),
            Instruction::JsrAbsolute(SELECT_RIGHT_FD_CHR_BANK_FOR_PAIR_ADDRESS),
            Instruction::Rts,
        ],
    )
    .unwrap();
    assert_eq!(wrapper.bytes, expected);
}

fn map_source_chr_page(source_page: u8) -> u8 {
    ((source_page & 0x1F) << 2) + 8
}
