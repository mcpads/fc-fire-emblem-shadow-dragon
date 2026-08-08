use anyhow::{Context, Result, ensure};
use retro_rp2a03::decode_bytes;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct TypedInstructionBinding {
    cpu_address: u16,
    cpu_address_hex: String,
    mnemonic: String,
    addressing_mode: String,
    operand: String,
    control_flow: String,
}

pub(crate) fn decode_rp2a03_sequence(
    bytes: &[u8],
    origin: u16,
    role: &str,
) -> Result<Vec<TypedInstructionBinding>> {
    let mut bindings = Vec::new();
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let instruction = decode_bytes(&bytes[offset..])
            .with_context(|| format!("decode {role} at +0x{offset:X} through typed RP2A03 ISA"))?;
        ensure!(
            instruction.opcode_is_documented(),
            "{role} contains undocumented selector at +0x{offset:X}"
        );
        let address = origin
            .checked_add(offset as u16)
            .context("typed RP2A03 address overflow")?;
        bindings.push(TypedInstructionBinding {
            cpu_address: address,
            cpu_address_hex: format!("0x{address:04X}"),
            mnemonic: instruction.mnemonic().to_string(),
            addressing_mode: format!("{:?}", instruction.addressing_mode()),
            operand: format!("{:?}", instruction.operand()),
            control_flow: format!("{:?}", instruction.control_flow(address)),
        });
        offset += instruction.encoded_len();
    }
    ensure!(
        offset == bytes.len(),
        "{role} typed decode did not consume the full region"
    );
    Ok(bindings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_truncated_rp2a03_region() {
        let error = decode_rp2a03_sequence(&[0x4C, 0x00], 0x8000, "truncated_test").unwrap_err();
        assert!(error.to_string().contains("typed RP2A03 ISA"));
    }
}
