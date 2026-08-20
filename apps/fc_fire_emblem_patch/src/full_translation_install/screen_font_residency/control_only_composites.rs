//! 글리프를 출력하지 않고 빈 행 제어만 만드는 복합 상태를 원본에 결속한다.

use anyhow::{Context, Result, ensure};

use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    fixed_string_consumers::FixedStringConsumerInspection, rom::Rom,
    typed_source::decode_rp2a03_sequence,
};

const FIXED_STRING_BANK: u8 = 0x0B;

pub(super) const CONTROL_ONLY_RETAINED_COMPOSITE_STATES: [u8; 2] = [0x11, 0x17];

const STATE_ELEVEN_HANDLER: [u8; 51] = [
    0xA9, 0x08, 0x8D, 0xCF, 0x05, 0xA9, 0x08, 0x8D, 0xD0, 0x05, 0xA9, 0x70, 0x85, 0x70, 0xA2, 0x90,
    0xA5, 0x8F, 0x38, 0xE5, 0x64, 0xC9, 0x08, 0x90, 0x02, 0xA2, 0x10, 0x86, 0x71, 0x20, 0x3C, 0x8E,
    0xA0, 0x03, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0x88, 0xD0, 0xF7, 0xA9, 0xEF, 0x9D, 0x51, 0x04,
    0x4C, 0x39, 0x8F,
];
const STATE_SEVENTEEN_HANDLER: [u8; 40] = [
    0xA9, 0x12, 0x8D, 0xCF, 0x05, 0xA9, 0x12, 0x8D, 0xD0, 0x05, 0xA9, 0x50, 0x85, 0x70, 0xA9, 0x40,
    0x85, 0x71, 0x20, 0x3C, 0x8E, 0xA0, 0x08, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0x88, 0xD0, 0xF9,
    0xA9, 0xEF, 0x9D, 0x51, 0x04, 0x4C, 0x39, 0x8F,
];

struct ControlOnlyCompositeSpec {
    state: u8,
    handler: u16,
    expected: &'static [u8],
    producers: &'static [(u8, u16, u8)],
}

const CONTROL_ONLY_COMPOSITES: [ControlOnlyCompositeSpec; 2] = [
    ControlOnlyCompositeSpec {
        state: 0x11,
        handler: 0x8891,
        expected: &STATE_ELEVEN_HANDLER,
        producers: &[(0x06, 0xAF2A, 0x20)],
    },
    ControlOnlyCompositeSpec {
        state: 0x17,
        handler: 0x89FD,
        expected: &STATE_SEVENTEEN_HANDLER,
        producers: &[
            (0x06, 0x86CC, 0x4C),
            (0x06, 0x8A8F, 0x4C),
            (0x06, 0x9DF1, 0x20),
            (0x06, 0xA727, 0x20),
        ],
    },
];

pub(super) fn bind_control_only_composite_lifetimes(
    source: &Rom,
    inspection: &FixedStringConsumerInspection,
) -> Result<()> {
    let states = CONTROL_ONLY_COMPOSITES
        .iter()
        .map(|spec| spec.state)
        .collect::<Vec<_>>();
    ensure!(
        states == CONTROL_ONLY_RETAINED_COMPOSITE_STATES,
        "control-only composite state set changed"
    );

    for spec in &CONTROL_ONLY_COMPOSITES {
        ensure!(
            inspection.composite_handler_target(spec.state) == Some(spec.handler),
            "control-only composite {:02X} handler changed",
            spec.state
        );
        ensure!(
            !inspection
                .call_sites
                .iter()
                .any(|call| call.composite_state == spec.state),
            "control-only composite {:02X} gained a fixed-string appender",
            spec.state
        );
        let producers = inspection
            .composite_state_producers
            .iter()
            .filter(|producer| producer.state == spec.state)
            .map(|producer| {
                (
                    producer.prg_bank,
                    producer.cpu_address,
                    producer.transfer_opcode,
                )
            })
            .collect::<Vec<_>>();
        ensure!(
            producers == spec.producers,
            "control-only composite {:02X} producer family changed: {producers:?}",
            spec.state
        );

        let offset = switchable_cpu_to_file_offset(FIXED_STRING_BANK, spec.handler)?;
        let actual = source
            .data()
            .get(offset..offset + spec.expected.len())
            .context("control-only composite handler is outside the source ROM")?;
        ensure!(
            actual == spec.expected,
            "control-only composite {:02X} handler bytes changed",
            spec.state
        );
        decode_rp2a03_sequence(
            actual,
            spec.handler,
            "compose only ED row controls followed by EF",
        )?;
    }
    Ok(())
}
