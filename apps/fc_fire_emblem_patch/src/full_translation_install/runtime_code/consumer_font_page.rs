//! 비대사 복합 UI가 열려 있는 동안의 글꼴 페이지를 한 수명으로 관리한다.
//!
//! `$05E8`은 현재 화면이 아니라 마지막으로 요청한 합성기 ID다. 이를 전역 CHR
//! selector에서 계속 읽으면 화면을 닫은 뒤에도 예전 UI가 지형·전투·제목 그래픽을
//! 가로챈다. 반대로 `$E690`의 게시를 «다음 CHR 기록기 한 번»으로만 해석하면 같은
//! 화면의 커서 이동 재합성이 페이지를 다시 게시한 뒤 닫기 기록기를 가로챈다.
//!
//! 이 모듈은 그 두 수명을 분리한다. `$E690`과 이름 appender는 `$07FD`에 현재 페이지와
//! FE 소유 비트를 게시하고 즉시 적용한다. bank 0B의 유일한 복합 UI 열기 호출은 같은
//! route를 다시 적용하되 게시값을 소비하지 않는다. 전역 selector도 수명 동안 이 값을
//! 우선하므로 커서 이동이나 원본 FD/FE 갱신 뒤에 번역 페이지가 사라지지 않는다. 유일한
//! 닫기 호출만 게시값을 지우고 원본 그래픽 페이지 복원을 그대로 수행한다.

use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

pub(super) mod ending_lifetime;

use super::{
    DialogueRuntimeHook, DialogueRuntimeHookRole, DialogueRuntimeHookSite, RuntimeRoutine,
    chr_source_state::RIGHT_FD_SOURCE_SHADOW, next_address,
};
use crate::{
    full_translation_install::runtime_state_storage::CONSUMER_FONT_PAGE,
    mapper165::font_pair_projection::mapper_register_from_route,
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    sha1_hex,
    typed_source::decode_rp2a03_sequence,
};

const FIXED_BANK_BYTE_COUNT: usize = 16 * 1024;
const SOURCE_PRG_BANK_COUNT: usize = 16;
const UNIT_UI_BANK: u8 = 0x0B;

const COMPOSITE_STATE: u16 = 0x05E8;
const COMPOSITE_PAGE_ENTRY: u16 = 0xE690;
const COMPOSITE_PAGE_ENTRY_SOURCE: [u8; 12] = [
    0x8D, 0xE8, 0x05, 0xA9, 0x01, 0x85, 0x44, 0xA9, 0x0B, 0x4C, 0xFA, 0xC9,
];

const CENTRAL_RIGHT_FD_WRITER: u16 = 0xC9BE;
const JSR_ABSOLUTE_OPCODE: u8 = 0x20;
const JMP_ABSOLUTE_OPCODE: u8 = 0x4C;
const SCREEN_OPEN_RIGHT_FD_CALL: u16 = 0x928A;
const SCREEN_CLOSE_RIGHT_FD_CALL: u16 = 0x9324;
const SCREEN_OPEN_SEQUENCE_ADDRESS: u16 = 0x927B;
const SCREEN_OPEN_SEQUENCE: [u8; 18] = [
    0xA9, 0x06, 0x85, 0x44, 0x20, 0xFA, 0xC9, 0x20, 0xF5, 0xE6, 0x20, 0x0D, 0xC7, 0xA9, 0x00, 0x20,
    0xBE, 0xC9,
];
const SCREEN_CLOSE_SEQUENCE_ADDRESS: u16 = 0x931C;
const SCREEN_CLOSE_SEQUENCE: [u8; 14] = [
    0x20, 0x0D, 0xC7, 0xA4, 0x99, 0xB9, 0xE4, 0xC1, 0x20, 0xBE, 0xC9, 0x20, 0x0C, 0xE7,
];

const MAP_MENU_COMPOSITE_STATE: u8 = 0x03;
const UNIT_SUMMARY_COMPOSITE_STATE: u8 = 0x04;
const UNIT_COMMAND_COMPOSITE_STATE: u8 = 0x05;
const ATTACK_WEAPON_SELECTION_COMPOSITE_STATE: u8 = 0x06;
const UNIT_ITEM_LIST_COMPOSITE_STATE: u8 = 0x07;
const ITEM_ACTION_COMPOSITE_STATE: u8 = 0x09;
const UNIT_STATUS_COMPOSITE_STATE: u8 = 0x0F;
const CHAPTER_SAVE_OFFER_COMPOSITE_STATE: u8 = 0x1C;
#[derive(Clone, Copy)]
pub(in crate::full_translation_install) struct ConsumerFontPageRoutes {
    pub(in crate::full_translation_install) unit_command: u8,
    pub(in crate::full_translation_install) map_menu: u8,
    pub(in crate::full_translation_install) ending_record: u8,
    pub(in crate::full_translation_install) chapter_save_offer: u8,
    pub(in crate::full_translation_install) catalog: [u8; 2],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FontPageRole {
    UnitCommand,
    MapMenu,
    ChapterSaveOffer,
    CatalogDefault,
}

const COMPOSITE_PAGE_ROUTES: [(u8, FontPageRole); 6] = [
    (MAP_MENU_COMPOSITE_STATE, FontPageRole::MapMenu),
    (UNIT_COMMAND_COMPOSITE_STATE, FontPageRole::UnitCommand),
    (
        ATTACK_WEAPON_SELECTION_COMPOSITE_STATE,
        FontPageRole::CatalogDefault,
    ),
    (UNIT_ITEM_LIST_COMPOSITE_STATE, FontPageRole::CatalogDefault),
    (ITEM_ACTION_COMPOSITE_STATE, FontPageRole::CatalogDefault),
    (
        CHAPTER_SAVE_OFFER_COMPOSITE_STATE,
        FontPageRole::ChapterSaveOffer,
    ),
];

const EXPECTED_DIRECT_COMPOSITE_PRODUCER_COUNT: usize = 50;
const EXPECTED_DIRECT_COMPOSITE_PRODUCER_SHA1: &str = "eba4ee041d3af03bd5c2d71cc443e81fb01590a1";
const EXPECTED_FONT_PAGE_STATE_PRODUCERS: [CompositeStateProducer; 7] = [
    CompositeStateProducer::new(0x06, 0x903C, 0x20, UNIT_COMMAND_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0x90AF, 0x20, ATTACK_WEAPON_SELECTION_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0x93E2, 0x4C, UNIT_ITEM_LIST_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0x941D, 0x20, ITEM_ACTION_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0x9EB4, 0x20, UNIT_ITEM_LIST_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xA30D, 0x4C, MAP_MENU_COMPOSITE_STATE),
    CompositeStateProducer::new(0x06, 0xB78A, 0x4C, CHAPTER_SAVE_OFFER_COMPOSITE_STATE),
];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CompositeStateProducer {
    prg_bank: u8,
    cpu_address: u16,
    transfer_opcode: u8,
    state: u8,
}

impl CompositeStateProducer {
    const fn new(prg_bank: u8, cpu_address: u16, transfer_opcode: u8, state: u8) -> Self {
        Self {
            prg_bank,
            cpu_address,
            transfer_opcode,
            state,
        }
    }
}

impl FontPageRole {
    fn mapper_route(self, pages: ConsumerFontPageRoutes) -> u8 {
        match self {
            Self::UnitCommand => pages.unit_command,
            Self::MapMenu => pages.map_menu,
            Self::ChapterSaveOffer => pages.chapter_save_offer,
            Self::CatalogDefault => pages.catalog[0],
        }
    }
}

impl ConsumerFontPageRoutes {
    fn all(self) -> [u8; 6] {
        [
            self.unit_command,
            self.map_menu,
            self.ending_record,
            self.chapter_save_offer,
            self.catalog[0],
            self.catalog[1],
        ]
    }

    fn validate(self) -> Result<()> {
        let routes = self.all();
        ensure!(
            routes.iter().all(|route| *route != 0),
            "consumer font page uses the empty sentinel as a mapper route"
        );
        let mapper_registers = routes.map(mapper_register_from_route);
        ensure!(
            mapper_registers.into_iter().collect::<BTreeSet<_>>().len() == mapper_registers.len(),
            "consumer font page maps two roles to the same translated page"
        );
        ensure!(
            routes.iter().all(|route| {
                let mapper_register = mapper_register_from_route(*route);
                mapper_register != 0 && mapper_register & 0x03 == 0 && *route & !0xFD == 0
            }),
            "consumer font page is not an encoded FD/FE page route"
        );
        Ok(())
    }
}

/// 합성 진입과 화면 열기·닫기의 중앙 FD 공급을 원본과 후보 양쪽에 묶는다.
/// bank 0B에서 `$C9BE`를 직접 부르는 곳이 이 두 자리뿐이어야, 열기만 현재 UI
/// 페이지를 적용하고 닫기는 원본 selector 사슬로 복귀한다는 분모가 닫힌다.
pub(super) fn bind_consumer_font_page_lifetime(source: &Rom, candidate: &Rom) -> Result<()> {
    bind_direct_composite_state_producers(source, candidate)?;
    for (image_role, rom) in [("source", source), ("candidate", candidate)] {
        let entry = fixed_slice(rom, COMPOSITE_PAGE_ENTRY, COMPOSITE_PAGE_ENTRY_SOURCE.len())?;
        ensure!(
            entry == COMPOSITE_PAGE_ENTRY_SOURCE,
            "{image_role} composite page entry changed at {COMPOSITE_PAGE_ENTRY:04X}"
        );
        decode_rp2a03_sequence(
            entry,
            COMPOSITE_PAGE_ENTRY,
            "publish and dispatch one composite request",
        )?;

        for (address, expected, role) in [
            (
                SCREEN_OPEN_SEQUENCE_ADDRESS,
                SCREEN_OPEN_SEQUENCE.as_slice(),
                "open one composite screen and supply its central right-FD page",
            ),
            (
                SCREEN_CLOSE_SEQUENCE_ADDRESS,
                SCREEN_CLOSE_SEQUENCE.as_slice(),
                "close one composite screen and restore its central right-FD page",
            ),
        ] {
            let actual = switchable_slice(rom, UNIT_UI_BANK, address, expected.len())?;
            ensure!(
                actual == expected,
                "{image_role} {role} changed at {UNIT_UI_BANK:02X}:{address:04X}"
            );
            decode_rp2a03_sequence(actual, address, role)?;
        }

        let transfers = direct_transfer_sites_in_bank(rom, UNIT_UI_BANK, CENTRAL_RIGHT_FD_WRITER)?;
        ensure!(
            transfers
                == [
                    (SCREEN_OPEN_RIGHT_FD_CALL, JSR_ABSOLUTE_OPCODE),
                    (SCREEN_CLOSE_RIGHT_FD_CALL, JSR_ABSOLUTE_OPCODE),
                ],
            "{image_role} bank {UNIT_UI_BANK:02X} central right-FD direct-transfer census changed: {transfers:?}"
        );
    }
    Ok(())
}

/// `$E690`의 모든 직접 호출은 바로 앞 `LDA #state`와 한 명령 단위로 결속한다.
/// 페이지를 게시하는 상태의 생산자 집합도 별도로 고정해, 재사용 상태가 늘거나 새
/// 호출자가 생겼을 때 화면 역할을 추측한 채 통과하지 못하게 한다.
fn bind_direct_composite_state_producers(source: &Rom, candidate: &Rom) -> Result<()> {
    let source_catalog = scan_direct_composite_state_producers(source)?;
    ensure!(
        source_catalog.len() == EXPECTED_DIRECT_COMPOSITE_PRODUCER_COUNT,
        "direct composite-state producer population changed"
    );
    let mut identity = Vec::with_capacity(source_catalog.len() * 5);
    for producer in &source_catalog {
        identity.push(producer.prg_bank);
        identity.extend_from_slice(&producer.cpu_address.to_le_bytes());
        identity.push(producer.transfer_opcode);
        identity.push(producer.state);
    }
    ensure!(
        sha1_hex(&identity) == EXPECTED_DIRECT_COMPOSITE_PRODUCER_SHA1,
        "direct composite-state producer catalog changed"
    );
    ensure!(
        scan_direct_composite_state_producers(candidate)? == source_catalog,
        "candidate direct composite-state producer catalog changed"
    );

    let font_page_states = COMPOSITE_PAGE_ROUTES
        .iter()
        .map(|(state, _)| *state)
        .collect::<BTreeSet<_>>();
    let page_producers = source_catalog
        .into_iter()
        .filter(|producer| font_page_states.contains(&producer.state))
        .collect::<Vec<_>>();
    ensure!(
        page_producers == EXPECTED_FONT_PAGE_STATE_PRODUCERS,
        "font-page composite-state producer routes changed: {page_producers:?}"
    );
    Ok(())
}

fn scan_direct_composite_state_producers(rom: &Rom) -> Result<Vec<CompositeStateProducer>> {
    let prg = rom
        .prg()
        .get(..SOURCE_PRG_BANK_COUNT * FIXED_BANK_BYTE_COUNT)
        .context("image has fewer than sixteen source PRG banks")?;
    let target = COMPOSITE_PAGE_ENTRY.to_le_bytes();
    let mut producers = Vec::new();
    for (bank_index, bank) in prg.chunks_exact(FIXED_BANK_BYTE_COUNT).enumerate() {
        for offset in 2..bank.len() - 2 {
            let opcode = bank[offset];
            if ![JSR_ABSOLUTE_OPCODE, JMP_ABSOLUTE_OPCODE].contains(&opcode)
                || bank[offset + 1..offset + 3] != target
            {
                continue;
            }
            ensure!(
                bank[offset - 2] == 0xA9,
                "direct composite-state transfer has no immediate state producer at bank {bank_index:02X} offset {offset:04X}"
            );
            let cpu_address =
                0x8000 + u16::try_from(offset).context("composite producer offset exceeds u16")?;
            decode_rp2a03_sequence(
                &bank[offset - 2..offset + 3],
                cpu_address - 2,
                "load one composite state and transfer to its fixed writer",
            )?;
            producers.push(CompositeStateProducer::new(
                u8::try_from(bank_index).context("composite producer bank exceeds u8")?,
                cpu_address,
                opcode,
                bank[offset - 1],
            ));
        }
    }
    producers.sort_unstable();
    Ok(producers)
}

/// 원본 `STA $05E8`를 같은 길이의 호출로 바꾼다. 호출 뒤의 `LDA #$01`이 A와
/// 플래그를 즉시 새로 정하므로 게시기는 X/Y만 건드리지 않으면 된다.
pub(super) fn page_publisher_hook(publisher: u16) -> Result<DialogueRuntimeHook> {
    Ok(DialogueRuntimeHook {
        role: DialogueRuntimeHookRole::ConsumerFontPagePublisher,
        write_role: "consumer font page publisher hook",
        site: DialogueRuntimeHookSite::Fixed(COMPOSITE_PAGE_ENTRY),
        bytes: assemble_at(COMPOSITE_PAGE_ENTRY, &[Instruction::JsrAbsolute(publisher)])?,
    })
}

/// 복합 UI의 열기·닫기 두 호출만 각 수명 routine으로 보낸다. 둘 다 원본과 같은
/// 3바이트 `JSR`이어서 주변 명령 경계와 반환 주소를 바꾸지 않는다.
pub(super) fn screen_lifetime_hooks(open: u16, close: u16) -> Result<[DialogueRuntimeHook; 2]> {
    Ok([
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::ConsumerFontPageOpen,
            write_role: "consumer font page screen-open hook",
            site: DialogueRuntimeHookSite::Switchable {
                bank: UNIT_UI_BANK,
                address: SCREEN_OPEN_RIGHT_FD_CALL,
            },
            bytes: assemble_at(SCREEN_OPEN_RIGHT_FD_CALL, &[Instruction::JsrAbsolute(open)])?,
        },
        DialogueRuntimeHook {
            role: DialogueRuntimeHookRole::ConsumerFontPageClose,
            write_role: "consumer font page screen-close hook",
            site: DialogueRuntimeHookSite::Switchable {
                bank: UNIT_UI_BANK,
                address: SCREEN_CLOSE_RIGHT_FD_CALL,
            },
            bytes: assemble_at(
                SCREEN_CLOSE_RIGHT_FD_CALL,
                &[Instruction::JsrAbsolute(close)],
            )?,
        },
    ])
}

/// A의 계획된 FD/FE route를 현재 UI 페이지로 게시하고 즉시 적용한다.
/// 합성기와 이름 appender가 이 routine 하나를 공유하므로 첫 열기와 같은 화면 재그리기가
/// 서로 다른 페이지 적용 규칙을 가질 수 없다. 알 수 없는 값은 0으로 지우고 매퍼를
/// 건드리지 않는다.
pub(super) fn build_consumer_font_page_activation(
    origin: u16,
    apply_route: u16,
    pages: ConsumerFontPageRoutes,
) -> Result<RuntimeRoutine> {
    pages.validate()?;
    let mut instructions = Vec::new();
    let mut valid_jumps = Vec::new();
    for page in pages.all() {
        instructions.push(Instruction::CmpImmediate(page));
        let jump = instructions.len();
        instructions.push(Instruction::BeqAbsolute(origin));
        valid_jumps.push(jump);
    }
    instructions.extend([
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(CONSUMER_FONT_PAGE),
        Instruction::Rts,
    ]);

    let valid = next_address(origin, &instructions)?;
    for jump in valid_jumps {
        instructions[jump] = Instruction::BeqAbsolute(valid);
    }
    instructions.extend([
        Instruction::StaAbsolute(CONSUMER_FONT_PAGE),
        Instruction::JmpAbsolute(apply_route),
    ]);

    Ok(RuntimeRoutine {
        role: "consumer font page activation",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// 이번 합성에 필요한 고정 페이지를 게시한다. 요약·상태처럼 이름별 페이지가
/// 필요한 화면은 0으로 시작하고, 합성 중 unit/enemy appender가 실제 페이지를
/// 게시한다. 아이템·병종·동작 문자열은 모든 카탈로그 페이지에서 같은 코드를 쓰므로
/// 기본 카탈로그 페이지를 게시한다.
pub(super) fn build_composite_font_page_publisher(
    origin: u16,
    activation: u16,
    pages: ConsumerFontPageRoutes,
) -> Result<RuntimeRoutine> {
    pages.validate()?;
    ensure!(
        [UNIT_SUMMARY_COMPOSITE_STATE, UNIT_STATUS_COMPOSITE_STATE]
            .into_iter()
            .all(|dynamic_state| COMPOSITE_PAGE_ROUTES
                .iter()
                .all(|(state, _)| *state != dynamic_state)),
        "a name-dependent consumer received a static page request"
    );
    let mut instructions = vec![Instruction::StaAbsolute(COMPOSITE_STATE)];
    let mut route_jumps = Vec::with_capacity(COMPOSITE_PAGE_ROUTES.len());
    for (state, page) in COMPOSITE_PAGE_ROUTES {
        instructions.push(Instruction::CmpImmediate(state));
        let jump = instructions.len();
        instructions.push(Instruction::BeqAbsolute(origin));
        route_jumps.push((jump, page));
    }
    instructions.extend([
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(CONSUMER_FONT_PAGE),
        Instruction::Rts,
    ]);
    for page in [
        FontPageRole::CatalogDefault,
        FontPageRole::UnitCommand,
        FontPageRole::MapMenu,
        FontPageRole::ChapterSaveOffer,
    ] {
        let target = next_address(origin, &instructions)?;
        for (jump, page_role) in &route_jumps {
            if *page_role == page {
                instructions[*jump] = Instruction::BeqAbsolute(target);
            }
        }
        instructions.extend([
            Instruction::LdaImmediate(page.mapper_route(pages)),
            Instruction::JmpAbsolute(activation),
        ]);
    }

    Ok(RuntimeRoutine {
        role: "composite consumer font page publisher",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// bank 0B의 복합 UI 열기에서 현재 route를 다시 적용한다. 게시값은 닫기 경계까지
/// 유지하며, 유효한 route가 없으면 원본 `LDA #0; JSR $C9BE` 의미로 돌아간다.
pub(super) fn build_consumer_font_page_open(
    origin: u16,
    activation: u16,
) -> Result<RuntimeRoutine> {
    let mut instructions = vec![
        Instruction::LdaAbsolute(CONSUMER_FONT_PAGE),
        Instruction::JsrAbsolute(activation),
        Instruction::CmpImmediate(0),
    ];
    let no_page = instructions.len();
    instructions.push(Instruction::BeqAbsolute(origin));
    instructions.extend([
        Instruction::LdaImmediate(0),
        Instruction::StaZeroPage(RIGHT_FD_SOURCE_SHADOW),
        Instruction::Rts,
    ]);
    let no_page_target = next_address(origin, &instructions)?;
    instructions[no_page] = Instruction::BeqAbsolute(no_page_target);
    instructions.extend([
        Instruction::LdaImmediate(0),
        Instruction::JmpAbsolute(CENTRAL_RIGHT_FD_WRITER),
    ]);

    Ok(RuntimeRoutine {
        role: "consumer font page screen open",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

/// 복합 UI 닫기는 route를 먼저 지운 뒤 원본 A를 그대로 중앙 FD 기록기에 넘긴다.
/// 이후 전역 selector는 `$07FD=0`을 보고 대사 또는 기존 그래픽 사슬로 복귀하므로
/// 지형·전투·제목이 마지막 UI 페이지를 상속하지 않는다.
pub(super) fn build_consumer_font_page_close(
    origin: u16,
    restore_source_pair: u16,
) -> Result<RuntimeRoutine> {
    let instructions = [
        Instruction::Pha,
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(CONSUMER_FONT_PAGE),
        Instruction::Pla,
        Instruction::JsrAbsolute(CENTRAL_RIGHT_FD_WRITER),
        Instruction::JmpAbsolute(restore_source_pair),
    ];
    Ok(RuntimeRoutine {
        role: "consumer font page screen close",
        address: origin,
        bytes: assemble_at(origin, &instructions)?,
    })
}

fn fixed_slice(rom: &Rom, address: u16, len: usize) -> Result<&[u8]> {
    ensure!(address >= 0xC000, "fixed source address is below $C000");
    let start = rom
        .prg()
        .len()
        .checked_sub(FIXED_BANK_BYTE_COUNT)
        .and_then(|base| base.checked_add(usize::from(address - 0xC000)))
        .context("fixed source offset overflow")?;
    rom.prg()
        .get(start..start + len)
        .context("fixed source range is outside PRG")
}

fn switchable_slice(rom: &Rom, bank: u8, address: u16, len: usize) -> Result<&[u8]> {
    ensure!(
        (0x8000..0xC000).contains(&address),
        "switchable source address is outside $8000..$BFFF"
    );
    let start = usize::from(bank) * FIXED_BANK_BYTE_COUNT + usize::from(address - 0x8000);
    rom.prg()
        .get(start..start + len)
        .context("switchable source range is outside PRG")
}

fn direct_transfer_sites_in_bank(rom: &Rom, bank: u8, target: u16) -> Result<Vec<(u16, u8)>> {
    let bytes = switchable_slice(rom, bank, 0x8000, FIXED_BANK_BYTE_COUNT)?;
    let operand = target.to_le_bytes();
    let mut transfers = bytes
        .windows(3)
        .enumerate()
        .filter_map(|(offset, window)| {
            ([JSR_ABSOLUTE_OPCODE, JMP_ABSOLUTE_OPCODE].contains(&window[0])
                && window[1..] == operand)
                .then_some((
                    0x8000 + u16::try_from(offset).expect("16 KiB offset fits u16"),
                    window[0],
                ))
        })
        .collect::<Vec<_>>();
    transfers.sort_unstable();
    Ok(transfers)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: u16 = 0xF620;
    const APPLY_ROUTE: u16 = 0xF900;
    const RESTORE_SOURCE_PAIR: u16 = 0xF360;
    const REUSED_STATE_WITHOUT_FONT_OWNERSHIP: u8 = 0x20;
    const STATUS_ZERO: u8 = 0x02;

    fn pages() -> ConsumerFontPageRoutes {
        ConsumerFontPageRoutes {
            unit_command: 0xCC,
            map_menu: 0xD0,
            ending_record: 0xD9,
            chapter_save_offer: 0xD4,
            catalog: [0xDC, 0xE0],
        }
    }

    #[derive(Default)]
    struct RunResult {
        applied_route: Option<u8>,
        central_writer_value: Option<u8>,
        restored_source_pair: bool,
        a: u8,
    }

    struct TestCpu {
        memory: Box<[u8; 0x10000]>,
        a: u8,
        p: u8,
        sp: u8,
        pc: u16,
        applied_route: Option<u8>,
        central_writer_value: Option<u8>,
        restored_source_pair: bool,
    }

    impl TestCpu {
        fn new(
            memory: Box<[u8; 0x10000]>,
            routines: &[&RuntimeRoutine],
            entry: u16,
            a: u8,
            p: u8,
        ) -> Self {
            let mut memory = memory;
            for routine in routines {
                let start = usize::from(routine.address);
                let end = start + routine.bytes.len();
                memory[start..end].copy_from_slice(&routine.bytes);
            }
            Self {
                memory,
                a,
                p,
                sp: 0xFD,
                pc: entry,
                applied_route: None,
                central_writer_value: None,
                restored_source_pair: false,
            }
        }

        fn run(mut self) -> (Box<[u8; 0x10000]>, RunResult) {
            for _ in 0..256 {
                let opcode = self.read_pc();
                match opcode {
                    0x08 => self.push(self.p),
                    0x20 => {
                        let target = self.read_word_pc();
                        if target == APPLY_ROUTE {
                            self.applied_route = Some(self.a);
                        } else if target == CENTRAL_RIGHT_FD_WRITER {
                            self.central_writer_value = Some(self.a);
                        } else {
                            let return_address = self.pc.wrapping_sub(1);
                            self.push((return_address >> 8) as u8);
                            self.push(return_address as u8);
                            self.pc = target;
                        }
                    }
                    0x28 => {
                        self.p = self.pop();
                    }
                    0x48 => self.push(self.a),
                    0x4C => {
                        let target = self.read_word_pc();
                        if target == CENTRAL_RIGHT_FD_WRITER {
                            self.central_writer_value = Some(self.a);
                            return self.finish();
                        }
                        if target == APPLY_ROUTE {
                            self.applied_route = Some(self.a);
                            if self.sp == 0xFD {
                                return self.finish();
                            }
                            let low = self.pop();
                            let high = self.pop();
                            self.pc = u16::from_le_bytes([low, high]).wrapping_add(1);
                            continue;
                        }
                        if target == RESTORE_SOURCE_PAIR {
                            self.restored_source_pair = true;
                            return self.finish();
                        }
                        self.pc = target;
                    }
                    0x60 => {
                        if self.sp == 0xFD {
                            return self.finish();
                        }
                        let low = self.pop();
                        let high = self.pop();
                        self.pc = u16::from_le_bytes([low, high]).wrapping_add(1);
                    }
                    0x68 => {
                        self.a = self.pop();
                        self.set_zero(self.a == 0);
                    }
                    0x8D => {
                        let address = self.read_word_pc();
                        self.memory[usize::from(address)] = self.a;
                    }
                    0x85 => {
                        let address = self.read_pc();
                        self.memory[usize::from(address)] = self.a;
                    }
                    0xA9 => {
                        self.a = self.read_pc();
                        self.set_zero(self.a == 0);
                    }
                    0xAD => {
                        let address = self.read_word_pc();
                        self.a = self.memory[usize::from(address)];
                        self.set_zero(self.a == 0);
                    }
                    0xC9 => {
                        let value = self.read_pc();
                        self.set_zero(self.a == value);
                    }
                    0xF0 => {
                        let displacement = self.read_pc() as i8;
                        if self.p & STATUS_ZERO != 0 {
                            self.pc = self.pc.wrapping_add_signed(i16::from(displacement));
                        }
                    }
                    0xD0 => {
                        let displacement = self.read_pc() as i8;
                        if self.p & STATUS_ZERO == 0 {
                            self.pc = self.pc.wrapping_add_signed(i16::from(displacement));
                        }
                    }
                    other => panic!("test runtime reached unsupported opcode {other:02X}"),
                }
            }
            panic!("test runtime did not terminate");
        }

        fn finish(self) -> (Box<[u8; 0x10000]>, RunResult) {
            (
                self.memory,
                RunResult {
                    applied_route: self.applied_route,
                    central_writer_value: self.central_writer_value,
                    restored_source_pair: self.restored_source_pair,
                    a: self.a,
                },
            )
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

        fn set_zero(&mut self, set: bool) {
            if set {
                self.p |= STATUS_ZERO;
            } else {
                self.p &= !STATUS_ZERO;
            }
        }
    }

    fn run_routines(
        memory: Box<[u8; 0x10000]>,
        routines: &[&RuntimeRoutine],
        entry: u16,
        a: u8,
        p: u8,
    ) -> (Box<[u8; 0x10000]>, RunResult) {
        TestCpu::new(memory, routines, entry, a, p).run()
    }

    #[test]
    fn static_redraw_maps_immediately_and_screen_close_clears_the_page() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
        let publisher =
            build_composite_font_page_publisher(publisher_origin, activation.address, pages)
                .unwrap();
        let close_origin = publisher.address + u16::try_from(publisher.bytes.len()).unwrap();
        let close = build_consumer_font_page_close(close_origin, RESTORE_SOURCE_PAIR).unwrap();
        let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

        let (memory, first_draw) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            ITEM_ACTION_COMPOSITE_STATE,
            0xA5,
        );
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[0]);
        assert_eq!(first_draw.applied_route, Some(pages.catalog[0]));

        // 커서 이동이 같은 합성기를 다시 부르면 페이지를 즉시 다시 고르되, 닫기
        // 경계가 올 때까지 현재 UI 수명 안에 남는다.
        let (memory, redraw) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            ITEM_ACTION_COMPOSITE_STATE,
            0x24,
        );
        assert_eq!(redraw.applied_route, Some(pages.catalog[0]));
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[0]);

        let restore_page = 0x19;
        let (memory, closed) = run_routines(memory, &[&close], close.address, restore_page, 0x64);
        assert_eq!(closed.central_writer_value, Some(restore_page));
        assert!(closed.restored_source_pair);
        assert_eq!(closed.applied_route, None);
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
    }

    #[test]
    fn dynamic_name_page_can_change_during_one_open_screen() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let memory = vec![0; 0x10000].into_boxed_slice().try_into().unwrap();

        let (memory, first_name) = run_routines(
            memory,
            &[&activation],
            activation.address,
            pages.catalog[1],
            0x22,
        );
        assert_eq!(first_name.applied_route, Some(pages.catalog[1]));
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[1]);

        let (memory, next_name) = run_routines(
            memory,
            &[&activation],
            activation.address,
            pages.catalog[0],
            0x24,
        );
        assert_eq!(next_name.applied_route, Some(pages.catalog[0]));
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.catalog[0]);
    }

    #[test]
    fn unsupported_composite_clears_the_previous_screen_page() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
        let publisher =
            build_composite_font_page_publisher(publisher_origin, activation.address, pages)
                .unwrap();
        let mut memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        memory[usize::from(CONSUMER_FONT_PAGE)] = pages.catalog[1];

        let (memory, result) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            0x0D,
            0x00,
        );

        assert_eq!(memory[usize::from(COMPOSITE_STATE)], 0x0D);
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
        assert_eq!(result.applied_route, None);
    }

    #[test]
    fn reused_state_20_never_claims_the_ending_font_page() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let publisher_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
        let publisher =
            build_composite_font_page_publisher(publisher_origin, activation.address, pages)
                .unwrap();

        let mut memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        memory[usize::from(CONSUMER_FONT_PAGE)] = pages.catalog[0];
        let (memory, result) = run_routines(
            memory,
            &[&activation, &publisher],
            publisher.address,
            REUSED_STATE_WITHOUT_FONT_OWNERSHIP,
            0,
        );
        assert_eq!(result.applied_route, None);
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
    }

    #[test]
    fn screen_open_reapplies_and_retains_the_page_until_close() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let open_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
        let open = build_consumer_font_page_open(open_origin, activation.address).unwrap();
        let mut memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        memory[usize::from(CONSUMER_FONT_PAGE)] = pages.unit_command;
        memory[usize::from(RIGHT_FD_SOURCE_SHADOW)] = 0x19;

        let (memory, result) =
            run_routines(memory, &[&activation, &open], open.address, 0x55, 0xA4);

        assert_eq!(result.applied_route, Some(pages.unit_command));
        assert_eq!(result.central_writer_value, None);
        assert_eq!(result.a, 0);
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], pages.unit_command);
        assert_eq!(memory[usize::from(RIGHT_FD_SOURCE_SHADOW)], 0);
    }

    #[test]
    fn invalid_page_is_cleared_and_screen_open_uses_the_source_writer() {
        let pages = pages();
        let activation = build_consumer_font_page_activation(ORIGIN, APPLY_ROUTE, pages).unwrap();
        let open_origin = ORIGIN + u16::try_from(activation.bytes.len()).unwrap();
        let open = build_consumer_font_page_open(open_origin, activation.address).unwrap();
        let mut memory: Box<[u8; 0x10000]> =
            vec![0; 0x10000].into_boxed_slice().try_into().unwrap();
        memory[usize::from(CONSUMER_FONT_PAGE)] = 0x7F;

        let (memory, result) =
            run_routines(memory, &[&activation, &open], open.address, 0x11, 0xA4);

        assert_eq!(result.central_writer_value, Some(0));
        assert_eq!(result.applied_route, None);
        assert_eq!(memory[usize::from(CONSUMER_FONT_PAGE)], 0);
    }

    #[test]
    fn page_roles_cannot_share_the_empty_sentinel_or_each_other() {
        let mut invalid = pages();
        invalid.map_menu = 0;
        assert!(
            invalid
                .validate()
                .unwrap_err()
                .to_string()
                .contains("empty sentinel")
        );

        invalid = pages();
        invalid.map_menu = invalid.unit_command;
        assert!(
            invalid
                .validate()
                .unwrap_err()
                .to_string()
                .contains("same translated page")
        );

        invalid = pages();
        invalid.map_menu |= 2;
        assert!(
            invalid
                .validate()
                .unwrap_err()
                .to_string()
                .contains("encoded FD/FE page route")
        );
    }
}
