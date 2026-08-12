use anyhow::{Context, Result, ensure};
use retro_rp2a03::{Instruction, Rp2A03, decode_bytes};
use serde::Serialize;
use typed_isa_core::{ControlAction, ControlBoundary, ControlTarget, StaticSemantics};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rp2a03DirectControlFlow {
    FallThrough {
        next: u16,
    },
    Branch {
        target: u16,
        fallthrough: Option<u16>,
    },
    Jump {
        target: Option<u16>,
    },
    Call {
        target: u16,
        return_address: u16,
    },
    Return,
    Interrupt,
    Stop,
}

pub(crate) fn rp2a03_direct_control_flow(
    instruction: &Instruction,
    address: u16,
) -> Result<Rp2a03DirectControlFlow> {
    let flow = Rp2A03::semantics(instruction, &address)
        .expect("RP2A03 static semantics are infallible")
        .control_flow;
    match flow.action {
        ControlAction::Continue => Ok(Rp2a03DirectControlFlow::FallThrough {
            next: flow
                .fallthrough
                .context("RP2A03 continuation has no fallthrough")?,
        }),
        ControlAction::Transfer {
            target: ControlTarget::Direct(target),
        } if flow.fallthrough.is_some() => Ok(Rp2a03DirectControlFlow::Branch {
            target,
            fallthrough: flow.fallthrough,
        }),
        ControlAction::Transfer {
            target: ControlTarget::Direct(target),
        } => Ok(Rp2a03DirectControlFlow::Jump {
            target: Some(target),
        }),
        ControlAction::Transfer {
            target: ControlTarget::Indirect(_),
        } => Ok(Rp2a03DirectControlFlow::Jump { target: None }),
        ControlAction::LinkedTransfer {
            target: ControlTarget::Direct(target),
            return_site,
        } => Ok(Rp2a03DirectControlFlow::Call {
            target,
            return_address: return_site,
        }),
        ControlAction::LinkedTransfer {
            target: ControlTarget::Indirect(_),
            ..
        } => anyhow::bail!("RP2A03 linked transfer has an indirect target"),
        ControlAction::Return { .. } => Ok(Rp2a03DirectControlFlow::Return),
        ControlAction::ExceptionReturn { .. } => Ok(Rp2a03DirectControlFlow::Interrupt),
        ControlAction::Boundary(ControlBoundary::Trap { .. }) => {
            Ok(Rp2a03DirectControlFlow::Interrupt)
        }
        ControlAction::Boundary(ControlBoundary::Stop { .. }) => Ok(Rp2a03DirectControlFlow::Stop),
        ControlAction::Boundary(
            ControlBoundary::Wait { .. } | ControlBoundary::ProfileExit { .. },
        ) => anyhow::bail!("RP2A03 produced an unsupported control boundary"),
    }
}

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
