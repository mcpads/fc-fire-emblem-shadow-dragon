//! 엔딩 전적·캐릭터 후일담이 실제로 보이는 구간에 글꼴 페이지를 결속한다.
//!
//! 전적 화면은 여러 화면이 재사용하는 합성기 상태가 아니라 바깥 엔딩 phase 1에서
//! 시작한다. 캐릭터 후일담은 주 대사 상태가 terminal이 된 뒤에도 바깥 phase 0x10이
//! 화면을 계속 보존한다. 두 표면의 실제 진입·이탈 명령을 훅으로 삼고, 이탈할 때
//! 임시 FD/FE 페이지를 함께 원본 source shadow로 되돌린다.

use anyhow::{Context, Result, ensure};

use super::super::{
    DialogueRuntimeHook, DialogueRuntimeHookRole, DialogueRuntimeHookSite, RuntimeRoutine,
    chr_source_state::{
        CHR_SOURCE_HIGH_BITS, RIGHT_FD_HELPER, RIGHT_FD_SOURCE_SHADOW, RIGHT_FE_HELPER,
        RIGHT_FE_SOURCE_SHADOW,
    },
};
use crate::{
    chapter_transition::ENDING_RECORD_PHASE_ADDRESS,
    dialogue_inventory::switchable_cpu_to_file_offset,
    full_translation_install::runtime_state_storage::CONSUMER_FONT_PAGE,
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

use super::super::transport::REQUEST_STATE;

const ENDING_BANK: u8 = 0x04;
const ENDING_RECORD_ENTER_SITE: u16 = 0xA3DC;
const ENDING_RECORD_EXIT_SITE: u16 = 0xA48F;
const ENDING_CHARACTER_EPILOGUE_EXIT_SITE: u16 = 0xA24B;

const ENDING_RECORD_ENTER_ANCHOR_ADDRESS: u16 = 0xA3D9;
const ENDING_RECORD_ENTER_ANCHOR: [u8; 7] = [0xCA, 0x10, 0xEE, 0xEE, 0x31, 0x77, 0x60];
const ENDING_RECORD_EXIT_ANCHOR_ADDRESS: u16 = 0xA48A;
const ENDING_RECORD_EXIT_ANCHOR: [u8; 9] = [0xA9, 0x00, 0x8D, 0x32, 0x77, 0xEE, 0x31, 0x77, 0x60];
const ENDING_CHARACTER_EPILOGUE_EXIT_ANCHOR_ADDRESS: u16 = 0xA242;
const ENDING_CHARACTER_EPILOGUE_EXIT_ANCHOR: [u8; 16] = [
    0xAD, 0x09, 0x78, 0xF0, 0x07, 0xA9, 0x40, 0x85, 0x2E, 0xEE, 0x31, 0x77, 0x20, 0xA6, 0xA2, 0x60,
];

const EXIT_CAVE_FALSE_TRANSFER_ONE_BANK: u8 = 0x05;
const EXIT_CAVE_FALSE_TRANSFER_ONE_ANCHOR_ADDRESS: u16 = 0x8078;
const EXIT_CAVE_FALSE_TRANSFER_ONE_ANCHOR: [u8; 28] = [
    0xD0, 0x07, 0xA9, 0x40, 0x8D, 0xF2, 0x06, 0xD0, 0x13, 0xA9, 0x01, 0x8D, 0xF2, 0x06, 0xD0, 0x0C,
    0xA9, 0x20, 0x8D, 0xF9, 0x06, 0xD0, 0x05, 0xA9, 0x40, 0x8D, 0xF5, 0x06,
];
const EXIT_CAVE_FALSE_TRANSFER_TWO_BANK: u8 = 0x0E;
const EXIT_CAVE_FALSE_TRANSFER_TWO_ANCHOR_ADDRESS: u16 = 0x854D;
const EXIT_CAVE_FALSE_TRANSFER_TWO_ANCHOR: [u8; 20] = [
    0xC9, 0xBF, 0xF0, 0x1B, 0xC9, 0xBE, 0xF0, 0x20, 0x84, 0xF9, 0x29, 0x1F, 0x0A, 0xA8, 0xB9, 0x07,
    0x87, 0x9D, 0x08, 0x06,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RawExitCaveTransferCandidate {
    bank: u8,
    cpu_address: u16,
    target: u16,
}

const EXPECTED_RAW_EXIT_CAVE_TRANSFER_CANDIDATES: [RawExitCaveTransferCandidate; 2] = [
    RawExitCaveTransferCandidate {
        bank: 0x05,
        cpu_address: 0x8089,
        target: 0xF98D,
    },
    RawExitCaveTransferCandidate {
        bank: 0x0E,
        cpu_address: 0x8554,
        target: 0xF984,
    },
];

/// One exact sixteen-byte source gap immediately before the dialogue lifecycle cave.
pub(in crate::full_translation_install) const ENDING_FONT_EXIT_HEAD_ORIGIN: u16 = 0xF980;
pub(in crate::full_translation_install) const ENDING_FONT_EXIT_HEAD_END: u16 = 0xF990;

pub(in crate::full_translation_install) struct EndingFontLifetimeRuntime {
    pub(in crate::full_translation_install) restore_source_pair: RuntimeRoutine,
    pub(in crate::full_translation_install) enter_ending_record: RuntimeRoutine,
    pub(in crate::full_translation_install) exit_tail: RuntimeRoutine,
    pub(in crate::full_translation_install) exit_head: RuntimeRoutine,
}

impl EndingFontLifetimeRuntime {
    pub(in crate::full_translation_install) fn reclaimed_support_routines(
        &self,
    ) -> [&RuntimeRoutine; 3] {
        [
            &self.restore_source_pair,
            &self.enter_ending_record,
            &self.exit_tail,
        ]
    }

    pub(in crate::full_translation_install) fn hooks(&self) -> Result<[DialogueRuntimeHook; 3]> {
        Ok([
            hook(
                DialogueRuntimeHookRole::EndingRecordFontPageEnter,
                "ending record font-page entry hook",
                ENDING_RECORD_ENTER_SITE,
                self.enter_ending_record.address,
            )?,
            hook(
                DialogueRuntimeHookRole::EndingRecordFontPageExit,
                "ending record font-page exit hook",
                ENDING_RECORD_EXIT_SITE,
                self.exit_head.address,
            )?,
            hook(
                DialogueRuntimeHookRole::EndingCharacterEpilogueFontPageExit,
                "ending character-epilogue font-page exit hook",
                ENDING_CHARACTER_EPILOGUE_EXIT_SITE,
                self.exit_head.address,
            )?,
        ])
    }
}

/// Binds the three real ending lifetime transitions and the only new fixed gap.
pub(in crate::full_translation_install) fn bind_ending_font_lifetime(
    source: &Rom,
    candidate: &Rom,
) -> Result<()> {
    for (address, expected, role) in [
        (
            ENDING_RECORD_ENTER_ANCHOR_ADDRESS,
            ENDING_RECORD_ENTER_ANCHOR.as_slice(),
            "enter the ending record phase",
        ),
        (
            ENDING_RECORD_EXIT_ANCHOR_ADDRESS,
            ENDING_RECORD_EXIT_ANCHOR.as_slice(),
            "leave the ending record phase",
        ),
        (
            ENDING_CHARACTER_EPILOGUE_EXIT_ANCHOR_ADDRESS,
            ENDING_CHARACTER_EPILOGUE_EXIT_ANCHOR.as_slice(),
            "leave the visible ending character epilogue",
        ),
    ] {
        for (image_role, rom) in [("source", source), ("candidate", candidate)] {
            ensure!(
                switchable_slice(rom, ENDING_BANK, address, expected.len())? == expected,
                "{image_role} ending font lifetime changed while trying to {role} at {ENDING_BANK:02X}:{address:04X}"
            );
        }
        decode_rp2a03_sequence(expected, address, role)?;
    }

    for (bank, address, expected, role) in [
        (
            EXIT_CAVE_FALSE_TRANSFER_ONE_BANK,
            EXIT_CAVE_FALSE_TRANSFER_ONE_ANCHOR_ADDRESS,
            EXIT_CAVE_FALSE_TRANSFER_ONE_ANCHOR.as_slice(),
            "first instruction-interior exit-cave transfer candidate",
        ),
        (
            EXIT_CAVE_FALSE_TRANSFER_TWO_BANK,
            EXIT_CAVE_FALSE_TRANSFER_TWO_ANCHOR_ADDRESS,
            EXIT_CAVE_FALSE_TRANSFER_TWO_ANCHOR.as_slice(),
            "second instruction-interior exit-cave transfer candidate",
        ),
    ] {
        for (image_role, rom) in [("source", source), ("candidate", candidate)] {
            ensure!(
                switchable_slice(rom, bank, address, expected.len())? == expected,
                "{image_role} {role} changed at {bank:02X}:{address:04X}"
            );
        }
        decode_rp2a03_sequence(expected, address, role)?;
    }

    for (image_role, rom) in [("source", source), ("candidate", candidate)] {
        ensure!(
            fixed_slice(rom, ENDING_FONT_EXIT_HEAD_ORIGIN, ENDING_FONT_EXIT_HEAD_END,)?
                .iter()
                .all(|byte| *byte == 0xFF),
            "{image_role} ending font exit-head cave is not exact FF"
        );
    }
    ensure!(
        raw_exit_cave_transfer_candidates(source)? == EXPECTED_RAW_EXIT_CAVE_TRANSFER_CANDIDATES,
        "source raw transfer candidates into the ending font exit-head cave changed"
    );
    Ok(())
}

/// Builds one shared pair restore, the phase-1 entry, and both phase exits.
pub(in crate::full_translation_install) fn build_ending_font_lifetime(
    reclaimed_support_origin: u16,
    consumer_page_activation: u16,
    ending_record_route: u8,
) -> Result<EndingFontLifetimeRuntime> {
    let restore_source_pair = build_restore_source_pair(reclaimed_support_origin)?;
    let enter_origin = end_address(&restore_source_pair)?;
    let enter_ending_record =
        build_enter_ending_record(enter_origin, consumer_page_activation, ending_record_route)?;
    let exit_tail_origin = end_address(&enter_ending_record)?;
    let exit_tail = build_exit_tail(exit_tail_origin, restore_source_pair.address)?;
    let exit_head = build_exit_head(exit_tail.address)?;

    ensure!(
        end_address(&exit_head)? == ENDING_FONT_EXIT_HEAD_END,
        "ending font exit head no longer exactly fills its sixteen-byte cave"
    );
    Ok(EndingFontLifetimeRuntime {
        restore_source_pair,
        enter_ending_record,
        exit_tail,
        exit_head,
    })
}

fn build_restore_source_pair(origin: u16) -> Result<RuntimeRoutine> {
    let instructions = [
        Instruction::Php,
        Instruction::Pha,
        Instruction::LdaZeroPage(RIGHT_FD_SOURCE_SHADOW),
        Instruction::OraZeroPage(CHR_SOURCE_HIGH_BITS),
        Instruction::JsrAbsolute(RIGHT_FD_HELPER),
        Instruction::LdaZeroPage(RIGHT_FE_SOURCE_SHADOW),
        Instruction::OraZeroPage(CHR_SOURCE_HIGH_BITS),
        Instruction::JsrAbsolute(RIGHT_FE_HELPER),
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ];
    Ok(RuntimeRoutine {
        role: "restore source right-FD and right-FE CHR pair",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

fn build_enter_ending_record(
    origin: u16,
    consumer_page_activation: u16,
    ending_record_route: u8,
) -> Result<RuntimeRoutine> {
    let instructions = [
        Instruction::IncAbsolute(ENDING_RECORD_PHASE_ADDRESS),
        Instruction::Php,
        Instruction::Pha,
        Instruction::LdaImmediate(ending_record_route),
        Instruction::JsrAbsolute(consumer_page_activation),
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ];
    Ok(RuntimeRoutine {
        role: "enter ending record font-page lifetime",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

fn build_exit_head(exit_tail: u16) -> Result<RuntimeRoutine> {
    let instructions = [
        Instruction::IncAbsolute(ENDING_RECORD_PHASE_ADDRESS),
        Instruction::Php,
        Instruction::Pha,
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(CONSUMER_FONT_PAGE),
        Instruction::StaAbsolute(REQUEST_STATE),
        Instruction::JmpAbsolute(exit_tail),
    ];
    Ok(RuntimeRoutine {
        role: "leave one ending font-page lifetime",
        address: ENDING_FONT_EXIT_HEAD_ORIGIN,
        bytes: assemble_at(ENDING_FONT_EXIT_HEAD_ORIGIN, &instructions)?,
    })
}

fn build_exit_tail(origin: u16, restore_source_pair: u16) -> Result<RuntimeRoutine> {
    let instructions = [
        Instruction::JsrAbsolute(restore_source_pair),
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ];
    Ok(RuntimeRoutine {
        role: "finish one ending font-page exit",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

fn hook(
    role: DialogueRuntimeHookRole,
    write_role: &'static str,
    site: u16,
    target: u16,
) -> Result<DialogueRuntimeHook> {
    Ok(DialogueRuntimeHook {
        role,
        write_role,
        site: DialogueRuntimeHookSite::Switchable {
            bank: ENDING_BANK,
            address: site,
        },
        bytes: assemble_at(site, &[Instruction::JsrAbsolute(target)])?,
    })
}

fn end_address(routine: &RuntimeRoutine) -> Result<u16> {
    routine
        .address
        .checked_add(u16::try_from(routine.bytes.len()).context("ending routine length overflow")?)
        .context("ending routine address overflow")
}

fn switchable_slice(rom: &Rom, bank: u8, address: u16, len: usize) -> Result<&[u8]> {
    let offset = switchable_cpu_to_file_offset(bank, address)?;
    rom.data()
        .get(offset..offset + len)
        .context("ending font lifetime source range is outside ROM")
}

fn fixed_slice(rom: &Rom, start: u16, end: u16) -> Result<&[u8]> {
    ensure!(start >= 0xC000 && start <= end, "invalid fixed ending cave");
    let base = rom
        .prg()
        .len()
        .checked_sub(16 * 1024)
        .context("PRG is smaller than one fixed bank")?;
    rom.prg()
        .get(base + usize::from(start - 0xC000)..base + usize::from(end - 0xC000))
        .context("ending font lifetime fixed cave is outside ROM")
}

fn raw_exit_cave_transfer_candidates(rom: &Rom) -> Result<Vec<RawExitCaveTransferCandidate>> {
    const BANK_BYTE_COUNT: usize = 16 * 1024;
    let mut candidates = Vec::new();
    for (bank_index, bank) in rom.prg().chunks_exact(BANK_BYTE_COUNT).enumerate() {
        for (offset, window) in bank.windows(3).enumerate() {
            if !matches!(window[0], 0x20 | 0x4C) {
                continue;
            }
            let target = u16::from_le_bytes([window[1], window[2]]);
            if !(ENDING_FONT_EXIT_HEAD_ORIGIN..ENDING_FONT_EXIT_HEAD_END).contains(&target) {
                continue;
            }
            candidates.push(RawExitCaveTransferCandidate {
                bank: u8::try_from(bank_index).context("exit-cave candidate bank exceeds u8")?,
                cpu_address: 0x8000
                    + u16::try_from(offset).context("exit-cave candidate offset exceeds u16")?,
                target,
            });
        }
    }
    Ok(candidates)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rom::{HEADER_SIZE, Rom};

    const SUPPORT_ORIGIN: u16 = 0xF351;
    const ACTIVATION: u16 = 0xF620;
    const ENDING_ROUTE: u8 = 0xD9;
    const STATUS_ZERO: u8 = 0x02;
    const STATUS_NEGATIVE: u8 = 0x80;

    struct TestCpu {
        memory: Box<[u8; 0x10000]>,
        a: u8,
        p: u8,
        sp: u8,
        pc: u16,
        activation_route: Option<u8>,
        restored_pages: Vec<(u16, u8)>,
        local_restore: u16,
    }

    impl TestCpu {
        fn run(
            runtime: &EndingFontLifetimeRuntime,
            entry: u16,
            memory: Box<[u8; 0x10000]>,
            a: u8,
            p: u8,
        ) -> Self {
            let mut cpu = Self {
                memory,
                a,
                p,
                sp: 0xFD,
                pc: entry,
                activation_route: None,
                restored_pages: Vec::new(),
                local_restore: runtime.restore_source_pair.address,
            };
            for routine in runtime
                .reclaimed_support_routines()
                .into_iter()
                .chain([&runtime.exit_head])
            {
                let start = usize::from(routine.address);
                cpu.memory[start..start + routine.bytes.len()].copy_from_slice(&routine.bytes);
            }
            for _ in 0..128 {
                let opcode = cpu.read_pc();
                match opcode {
                    0x05 => {
                        let address = cpu.read_pc();
                        cpu.a |= cpu.memory[usize::from(address)];
                        cpu.set_nz(cpu.a);
                    }
                    0x08 => cpu.push(cpu.p),
                    0x20 => {
                        let target = cpu.read_word_pc();
                        if target == ACTIVATION {
                            cpu.activation_route = Some(cpu.a);
                            cpu.memory[usize::from(CONSUMER_FONT_PAGE)] = cpu.a;
                        } else if [RIGHT_FD_HELPER, RIGHT_FE_HELPER].contains(&target) {
                            cpu.restored_pages.push((target, cpu.a));
                        } else if target == cpu.local_restore {
                            let return_address = cpu.pc.wrapping_sub(1);
                            cpu.push((return_address >> 8) as u8);
                            cpu.push(return_address as u8);
                            cpu.pc = target;
                        } else {
                            panic!("unexpected test JSR target {target:04X}");
                        }
                    }
                    0x28 => cpu.p = cpu.pop(),
                    0x48 => cpu.push(cpu.a),
                    0x4C => cpu.pc = cpu.read_word_pc(),
                    0x60 => {
                        if cpu.sp == 0xFD {
                            return cpu;
                        }
                        let low = cpu.pop();
                        let high = cpu.pop();
                        cpu.pc = u16::from_le_bytes([low, high]).wrapping_add(1);
                    }
                    0x68 => {
                        cpu.a = cpu.pop();
                        cpu.set_nz(cpu.a);
                    }
                    0x8D => {
                        let address = cpu.read_word_pc();
                        cpu.memory[usize::from(address)] = cpu.a;
                    }
                    0xA5 => {
                        let address = cpu.read_pc();
                        cpu.a = cpu.memory[usize::from(address)];
                        cpu.set_nz(cpu.a);
                    }
                    0xA9 => {
                        cpu.a = cpu.read_pc();
                        cpu.set_nz(cpu.a);
                    }
                    0xEE => {
                        let address = cpu.read_word_pc();
                        let value = cpu.memory[usize::from(address)].wrapping_add(1);
                        cpu.memory[usize::from(address)] = value;
                        cpu.set_nz(value);
                    }
                    other => panic!("unsupported ending-lifetime opcode {other:02X}"),
                }
            }
            panic!("ending-lifetime test routine did not return")
        }

        fn read_pc(&mut self) -> u8 {
            let value = self.memory[usize::from(self.pc)];
            self.pc = self.pc.wrapping_add(1);
            value
        }

        fn read_word_pc(&mut self) -> u16 {
            let low = self.read_pc();
            let high = self.read_pc();
            u16::from_le_bytes([low, high])
        }

        fn push(&mut self, value: u8) {
            self.memory[0x100 + usize::from(self.sp)] = value;
            self.sp = self.sp.wrapping_sub(1);
        }

        fn pop(&mut self) -> u8 {
            self.sp = self.sp.wrapping_add(1);
            self.memory[0x100 + usize::from(self.sp)]
        }

        fn set_nz(&mut self, value: u8) {
            self.p &= !(STATUS_ZERO | STATUS_NEGATIVE);
            if value == 0 {
                self.p |= STATUS_ZERO;
            }
            if value & STATUS_NEGATIVE != 0 {
                self.p |= STATUS_NEGATIVE;
            }
        }
    }

    fn runtime() -> EndingFontLifetimeRuntime {
        build_ending_font_lifetime(SUPPORT_ORIGIN, ACTIVATION, ENDING_ROUTE).unwrap()
    }

    #[test]
    fn phase_one_entry_owns_the_ending_page_and_preserves_the_increment_result() {
        let runtime = runtime();
        let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        let cpu = TestCpu::run(
            &runtime,
            runtime.enter_ending_record.address,
            memory,
            0xA6,
            0x21 | STATUS_NEGATIVE | STATUS_ZERO,
        );

        assert_eq!(cpu.memory[usize::from(ENDING_RECORD_PHASE_ADDRESS)], 1);
        assert_eq!(cpu.memory[usize::from(CONSUMER_FONT_PAGE)], ENDING_ROUTE);
        assert_eq!(cpu.activation_route, Some(ENDING_ROUTE));
        assert_eq!(cpu.a, 0xA6);
        assert_eq!(cpu.p, 0x21);
    }

    #[test]
    fn either_ending_exit_clears_both_owners_and_restores_the_source_pair() {
        for phase in [1, 0x10] {
            let runtime = runtime();
            let mut memory: Box<[u8; 0x10000]> =
                vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
            memory[usize::from(ENDING_RECORD_PHASE_ADDRESS)] = phase;
            memory[usize::from(CONSUMER_FONT_PAGE)] = ENDING_ROUTE;
            memory[usize::from(REQUEST_STATE)] = 3;
            memory[usize::from(RIGHT_FD_SOURCE_SHADOW)] = 0x12;
            memory[usize::from(RIGHT_FE_SOURCE_SHADOW)] = 0x17;
            memory[usize::from(CHR_SOURCE_HIGH_BITS)] = 0x20;

            let cpu = TestCpu::run(&runtime, runtime.exit_head.address, memory, 0xC7, 0xA1);
            assert_eq!(
                cpu.memory[usize::from(ENDING_RECORD_PHASE_ADDRESS)],
                phase + 1
            );
            assert_eq!(cpu.memory[usize::from(CONSUMER_FONT_PAGE)], 0);
            assert_eq!(cpu.memory[usize::from(REQUEST_STATE)], 0);
            assert_eq!(
                cpu.restored_pages,
                [(RIGHT_FD_HELPER, 0x32), (RIGHT_FE_HELPER, 0x37)]
            );
            assert_eq!(cpu.a, 0xC7);
            assert_eq!(cpu.p, 0x21);
        }
    }

    #[test]
    fn support_and_exit_routines_fit_their_owned_caves_exactly() {
        let runtime = runtime();
        assert_eq!(runtime.restore_source_pair.bytes.len(), 19);
        assert_eq!(runtime.enter_ending_record.bytes.len(), 13);
        assert_eq!(runtime.exit_tail.bytes.len(), 6);
        assert_eq!(end_address(&runtime.exit_tail).unwrap(), 0xF377);
        assert_eq!(runtime.exit_head.bytes.len(), 16);
        assert_eq!(
            end_address(&runtime.exit_head).unwrap(),
            ENDING_FONT_EXIT_HEAD_END
        );
    }

    #[test]
    fn source_lifetime_mutation_refuses_binding() {
        let mut source = synthetic_image();
        let candidate = synthetic_image();
        bind_ending_font_lifetime(
            &Rom::parse(source.clone()).unwrap(),
            &Rom::parse(candidate.clone()).unwrap(),
        )
        .unwrap();

        let offset = switchable_offset(ENDING_BANK, ENDING_RECORD_EXIT_ANCHOR_ADDRESS);
        source[offset] ^= 1;
        let error = bind_ending_font_lifetime(
            &Rom::parse(source).unwrap(),
            &Rom::parse(candidate).unwrap(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("leave the ending record phase"));
    }

    fn synthetic_image() -> Vec<u8> {
        let mut bytes = crate::test_support::synthetic_mapper165_rom_bytes(0xFF);
        for (bank, address, expected) in [
            (
                ENDING_BANK,
                ENDING_RECORD_ENTER_ANCHOR_ADDRESS,
                ENDING_RECORD_ENTER_ANCHOR.as_slice(),
            ),
            (
                ENDING_BANK,
                ENDING_RECORD_EXIT_ANCHOR_ADDRESS,
                ENDING_RECORD_EXIT_ANCHOR.as_slice(),
            ),
            (
                ENDING_BANK,
                ENDING_CHARACTER_EPILOGUE_EXIT_ANCHOR_ADDRESS,
                ENDING_CHARACTER_EPILOGUE_EXIT_ANCHOR.as_slice(),
            ),
            (
                EXIT_CAVE_FALSE_TRANSFER_ONE_BANK,
                EXIT_CAVE_FALSE_TRANSFER_ONE_ANCHOR_ADDRESS,
                EXIT_CAVE_FALSE_TRANSFER_ONE_ANCHOR.as_slice(),
            ),
            (
                EXIT_CAVE_FALSE_TRANSFER_TWO_BANK,
                EXIT_CAVE_FALSE_TRANSFER_TWO_ANCHOR_ADDRESS,
                EXIT_CAVE_FALSE_TRANSFER_TWO_ANCHOR.as_slice(),
            ),
        ] {
            let offset = switchable_offset(bank, address);
            bytes[offset..offset + expected.len()].copy_from_slice(expected);
        }
        bytes
    }

    fn switchable_offset(bank: u8, address: u16) -> usize {
        HEADER_SIZE + usize::from(bank) * 16 * 1024 + usize::from(address - 0x8000)
    }
}
