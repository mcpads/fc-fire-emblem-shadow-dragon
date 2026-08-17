//! 생성한 런타임 조각 안의 매퍼 쓰기를 typed ISA로 검사한다.
//!
//! 이 검사는 생성 조각의 직접·절대 인덱스 쓰기만 소유한다. whole-program 실행 분모와
//! 런타임 간접 주소 범위는 별도의 전역 관문이며 여기서 완료로 승격하지 않는다.

use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};
use retro_rp2a03::{
    AddressingMode, Location, MemoryAddress, Mnemonic, Operand, Rp2A03, decode_bytes,
};
use typed_isa_core::{AccessKind, StaticSemantics};

use super::{DialogueRuntimeCodePlan, DialogueRuntimeHookSite};
use crate::{
    mapper165::executable_mapper_writes::{Mapper165Register, decode_mapper165_write},
    typed_source::{Rp2a03DirectControlFlow, rp2a03_direct_control_flow},
};

pub(super) fn verify_planned_mapper_select_writes(plan: &DialogueRuntimeCodePlan) -> Result<()> {
    for routine in plan.code_routines.iter().chain(&plan.fixed_routines) {
        verify_generated_executable_mapper_select_pairs(
            routine.role,
            routine.address,
            &routine.bytes,
        )?;
    }
    for reclaimed in &plan.reclaimed_fixed_routines {
        ensure!(
            reclaimed.executable_byte_count <= reclaimed.routine.bytes.len(),
            "{} executable extent exceeds its overwrite extent",
            reclaimed.routine.role
        );
        ensure!(
            reclaimed.routine.bytes[reclaimed.executable_byte_count..]
                .iter()
                .all(|byte| *byte == 0xFF),
            "{} reclaimed-cave padding is not exact $FF",
            reclaimed.routine.role
        );
        verify_generated_executable_mapper_select_pairs(
            reclaimed.routine.role,
            reclaimed.routine.address,
            &reclaimed.routine.bytes[..reclaimed.executable_byte_count],
        )?;
    }
    for hook in &plan.hooks {
        let address = match hook.site {
            DialogueRuntimeHookSite::Fixed(address)
            | DialogueRuntimeHookSite::Switchable { address, .. } => address,
        };
        verify_generated_executable_mapper_select_pairs(hook.write_role, address, &hook.bytes)?;
    }
    Ok(())
}

/// Verifies the typed, generated plan's direct mapper165 writes. This is intentionally not the
/// global ExecutableImage denominator: runtime-computed indirect addresses remain a separate
/// fail-closed admission gate. Direct aliases and absolute-indexed ranges are handled here.
fn verify_generated_executable_mapper_select_pairs(
    role: &str,
    origin: u16,
    bytes: &[u8],
) -> Result<()> {
    let mut offset = 0;
    let mut decoded = Vec::new();
    while offset < bytes.len() {
        let address = origin
            .checked_add(u16::try_from(offset)?)
            .context("generated executable address overflow")?;
        let instruction = decode_bytes(&bytes[offset..])
            .with_context(|| format!("decode generated executable {role} at +{offset:04X}"))?;
        ensure!(
            instruction.opcode_is_documented(),
            "generated executable {role} contains an undocumented opcode at +{offset:04X}"
        );
        let semantics = Rp2A03::semantics(&instruction, &address)
            .expect("RP2A03 static semantics are infallible");
        let mut direct_value_write = false;
        for access in semantics.location_accesses {
            if access.kind != AccessKind::Write {
                continue;
            }
            let Location::Memory(memory) = access.location else {
                continue;
            };
            match memory {
                MemoryAddress::Direct(target) => match decode_mapper165_write(target) {
                    Some(Mapper165Register::BankSelect) => anyhow::bail!(
                        "generated executable {role} directly writes mapper-select alias ${target:04X} at +{offset:04X}"
                    ),
                    Some(Mapper165Register::BankData) => direct_value_write = true,
                    Some(register) => anyhow::bail!(
                        "generated executable {role} directly writes unexpected mapper165 {register:?} alias ${target:04X} at +{offset:04X}"
                    ),
                    None => {}
                },
                MemoryAddress::Effective {
                    mode: AddressingMode::AbsoluteX | AddressingMode::AbsoluteY,
                    operand: Operand::Word(base),
                } => {
                    ensure!(
                        !(0..=u8::MAX).any(|index| {
                            decode_mapper165_write(base.wrapping_add(u16::from(index))).is_some()
                        }),
                        "generated executable {role} has an absolute-indexed write whose effective range can enter mapper165 ports at +{offset:04X}"
                    );
                }
                MemoryAddress::Effective {
                    mode: AddressingMode::ZeroPageX | AddressingMode::ZeroPageY,
                    ..
                }
                | MemoryAddress::Stack => {}
                MemoryAddress::Effective {
                    mode:
                        AddressingMode::ZeroPageIndexedIndirectX
                        | AddressingMode::ZeroPageIndirectIndexedY,
                    ..
                }
                | MemoryAddress::Pointer { .. }
                | MemoryAddress::InterruptVector => {}
                MemoryAddress::Effective { mode, .. } => anyhow::bail!(
                    "generated executable {role} has an unhandled effective write mode {mode:?} at +{offset:04X}"
                ),
            }
        }
        decoded.push((address, instruction, direct_value_write));
        offset += instruction.encoded_len();
    }

    let mut bypass_targets = BTreeSet::new();
    for (address, instruction, _) in &decoded {
        match rp2a03_direct_control_flow(instruction, *address)? {
            Rp2a03DirectControlFlow::Branch { target, .. }
            | Rp2a03DirectControlFlow::Jump {
                target: Some(target),
            }
            | Rp2a03DirectControlFlow::Call { target, .. } => {
                bypass_targets.insert(target);
            }
            Rp2a03DirectControlFlow::Jump { target: None } => anyhow::bail!(
                "generated executable {role} contains an indirect jump whose mapper-pair entry effects are unresolved at ${address:04X}"
            ),
            _ => {}
        }
    }
    for (value_index, (value_address, _, direct_value_write)) in decoded.iter().enumerate() {
        if !*direct_value_write {
            continue;
        }
        let mut selector = None;
        for selector_index in (0..value_index).rev() {
            let (address, preceding, _) = decoded[selector_index];
            if preceding.mnemonic() == Mnemonic::Jsr
                && preceding.operand()
                    == Operand::Word(
                        crate::mapper165::selector_safety::SELECT_REGISTER_ROUTINE_ADDRESS,
                    )
            {
                selector = Some((selector_index, address));
                break;
            }
            if !matches!(
                rp2a03_direct_control_flow(&preceding, address)?,
                Rp2a03DirectControlFlow::FallThrough { .. }
            ) {
                break;
            }
        }
        let (selector_index, selector_address) = selector.with_context(|| {
            format!(
                "generated executable {role} writes canonical mapper-value address $8001 at ${value_address:04X} without a same-block common selector call"
            )
        })?;
        let after_selector = decoded
            .get(selector_index + 1)
            .map(|(address, _, _)| *address)
            .unwrap_or(*value_address);
        ensure!(
            !bypass_targets
                .range(after_selector..=*value_address)
                .next()
                .is_some(),
            "generated executable {role} can branch between common selector call ${selector_address:04X} and mapper-value write ${value_address:04X}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rp2a03::{Instruction, assemble_at};

    #[test]
    fn generated_code_cannot_bypass_the_common_selector_writer() {
        let direct = assemble_at(
            0xA000,
            &[
                Instruction::LdaImmediate(6),
                Instruction::StaAbsolute(0x8000),
                Instruction::Rts,
            ],
        )
        .unwrap();
        let error =
            verify_generated_executable_mapper_select_pairs("direct selector", 0xA000, &direct)
                .unwrap_err();

        assert!(error.to_string().contains("mapper-select alias"));
    }

    #[test]
    fn generated_code_cannot_write_an_unowned_mapper_register_alias() {
        let direct = assemble_at(
            0xA000,
            &[
                Instruction::LdaImmediate(0),
                Instruction::StaAbsolute(0xBFFE),
                Instruction::Rts,
            ],
        )
        .unwrap();

        let error = verify_generated_executable_mapper_select_pairs(
            "direct mirroring alias",
            0xA000,
            &direct,
        )
        .unwrap_err();

        assert!(error.to_string().contains("Mirroring alias $BFFE"));
    }

    #[test]
    fn generated_value_writes_require_a_same_block_selector_call() {
        let unpaired = assemble_at(
            0xA000,
            &[
                Instruction::LdaImmediate(0x20),
                Instruction::StaAbsolute(0x8001),
                Instruction::Rts,
            ],
        )
        .unwrap();
        assert!(
            verify_generated_executable_mapper_select_pairs("unpaired value", 0xA000, &unpaired)
                .unwrap_err()
                .to_string()
                .contains("without a same-block common selector call")
        );

        let paired = assemble_at(
            0xA000,
            &[
                Instruction::LdaImmediate(6),
                crate::mapper165::selector_safety::select_register_instruction(),
                Instruction::LdaImmediate(0x20),
                Instruction::StaAbsolute(0x8001),
                Instruction::Rts,
            ],
        )
        .unwrap();
        verify_generated_executable_mapper_select_pairs("paired value", 0xA000, &paired).unwrap();
    }

    #[test]
    fn generated_branches_cannot_enter_between_a_selector_and_its_value() {
        let bytes = assemble_at(
            0xA000,
            &[
                Instruction::BeqAbsolute(0xA009),
                Instruction::LdaImmediate(6),
                crate::mapper165::selector_safety::select_register_instruction(),
                Instruction::LdaImmediate(0x20),
                Instruction::StaAbsolute(0x8001),
                Instruction::Rts,
            ],
        )
        .unwrap();
        let error =
            verify_generated_executable_mapper_select_pairs("branch-bypass value", 0xA000, &bytes)
                .unwrap_err();

        assert!(error.to_string().contains("can branch between"));
    }

    #[test]
    fn generated_absolute_indexed_writes_cannot_reach_mapper_aliases() {
        let bytes = assemble_at(
            0xA000,
            &[Instruction::StaAbsoluteX(0x7F80), Instruction::Rts],
        )
        .unwrap();
        let error = verify_generated_executable_mapper_select_pairs(
            "indexed mapper candidate",
            0xA000,
            &bytes,
        )
        .unwrap_err();

        assert!(error.to_string().contains("absolute-indexed write"));
    }
}
