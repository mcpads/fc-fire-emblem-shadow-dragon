use std::collections::BTreeSet;

use anyhow::{Result, ensure};

use crate::{rom::Rom, typed_source::decode_rp2a03_sequence};

use super::{FixedVectorOpenControlEdge, fixed_source_bytes};

pub(super) const SOURCE_AUDIO_BANK: u8 = 0x0E;
pub(super) const SOURCE_AUDIO_BANK_CALL_START: u16 = 0xC1FB;
pub(super) const SOURCE_AUDIO_CALL_SITE: u16 = 0xC200;
pub(super) const SOURCE_AUDIO_ENTRY: u16 = 0x8000;
pub(super) const SOURCE_AUDIO_BANK_CALL_CODE: [u8; 14] = [
    0xA9,
    SOURCE_AUDIO_BANK,
    0x8D,
    0x00,
    0xA0,
    0x20,
    0x00,
    0x80,
    0xA5,
    0x29,
    0x8D,
    0x00,
    0xA0,
    0x60,
];

pub(super) fn bind_audio_bank_call(
    source: &Rom,
    open_control_edges: &mut BTreeSet<FixedVectorOpenControlEdge>,
) -> Result<BTreeSet<(u8, u16)>> {
    let edge = FixedVectorOpenControlEdge::SwitchableTarget {
        instruction: SOURCE_AUDIO_CALL_SITE,
        target: SOURCE_AUDIO_ENTRY,
    };
    let bytes = fixed_source_bytes(
        source,
        SOURCE_AUDIO_BANK_CALL_START,
        SOURCE_AUDIO_BANK_CALL_CODE.len(),
    )?;
    let edge_is_reached = open_control_edges.contains(&edge);
    let code_is_present = bytes == SOURCE_AUDIO_BANK_CALL_CODE;
    if !edge_is_reached && !code_is_present {
        return Ok(BTreeSet::new());
    }
    ensure!(
        code_is_present,
        "source fixed audio bank-call sequence changed"
    );
    ensure!(
        edge_is_reached,
        "source fixed audio bank-call sequence was not reached from a hardware vector"
    );
    decode_rp2a03_sequence(
        bytes,
        SOURCE_AUDIO_BANK_CALL_START,
        "source fixed audio bank call and shadow restore",
    )?;
    ensure!(
        open_control_edges.remove(&edge),
        "source fixed audio bank-call edge disappeared before binding"
    );
    Ok(BTreeSet::from([(SOURCE_AUDIO_BANK, SOURCE_AUDIO_ENTRY)]))
}
