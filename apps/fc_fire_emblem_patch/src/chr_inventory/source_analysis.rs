use super::*;

pub(super) fn find_adjacent_chr_write_candidate_groups(
    prg: &[u8],
) -> Vec<Mmc4ChrWriteCandidateGroup> {
    let mut candidates = MMC4_REGISTER_SPECS[1..5]
        .iter()
        .flat_map(|(register_address, _)| {
            find_absolute_write_candidates(prg, *register_address)
                .into_iter()
                .map(|candidate| (*register_address, candidate))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(_, candidate)| candidate.prg_offset);

    let mut candidate_runs = Vec::<Vec<(u16, AbsoluteWriteCandidate)>>::new();
    for candidate in candidates {
        let continues_run = candidate_runs
            .last()
            .and_then(|run| run.last())
            .is_some_and(|(_, previous)| {
                previous.prg_bank == candidate.1.prg_bank
                    && (3..=8).contains(&candidate.1.prg_offset.saturating_sub(previous.prg_offset))
            });
        if continues_run {
            candidate_runs.last_mut().unwrap().push(candidate);
        } else {
            candidate_runs.push(vec![candidate]);
        }
    }

    candidate_runs
        .into_iter()
        .filter(|run| run.len() >= 2)
        .map(|run| {
            let first = &run[0].1;
            let last = &run[run.len() - 1].1;
            let largest_gap_byte_count = run
                .windows(2)
                .map(|pair| pair[1].1.prg_offset - pair[0].1.prg_offset - 3)
                .max()
                .unwrap_or(0);
            Mmc4ChrWriteCandidateGroup {
                prg_bank: first.prg_bank,
                prg_bank_hex: first.prg_bank_hex.clone(),
                start_cpu_address: first.cpu_address,
                start_cpu_address_hex: first.cpu_address_hex.clone(),
                last_cpu_address: last.cpu_address,
                last_cpu_address_hex: last.cpu_address_hex.clone(),
                instruction_count: run.len(),
                largest_gap_byte_count,
                evidence: "same-bank absolute CHR-register writes separated by at most five bytes",
                disposition: "candidate_only_runtime_execution_or_disassembly_required",
                writes: run
                    .into_iter()
                    .map(|(register_address, candidate)| Mmc4ChrWriteCandidateSite {
                        cpu_address: candidate.cpu_address,
                        cpu_address_hex: candidate.cpu_address_hex,
                        prg_offset: candidate.prg_offset,
                        prg_offset_hex: candidate.prg_offset_hex,
                        opcode_hex: candidate.opcode_hex,
                        mnemonic: candidate.mnemonic,
                        register_address,
                        register_address_hex: format!("0x{register_address:04X}"),
                    })
                    .collect(),
            }
        })
        .collect()
}

pub(super) fn describe_mmc4_control_routines(prg: &[u8]) -> Result<Vec<Mmc4ControlRoutineReport>> {
    ensure!(
        prg.len() == PRG_SIZE,
        "unexpected PRG size for MMC4 control inventory"
    );
    MMC4_CONTROL_ROUTINES
        .iter()
        .map(|routine| {
            let prg_offset = fixed_bank_prg_offset(routine.cpu_address)?;
            let end = prg_offset + routine.expected.len();
            ensure!(
                prg[prg_offset..end] == *routine.expected,
                "MMC4 control routine {} at ${:04X} changed",
                routine.role,
                routine.cpu_address
            );
            Ok(Mmc4ControlRoutineReport {
                role: routine.role,
                cpu_address: routine.cpu_address,
                cpu_address_hex: format!("0x{:04X}", routine.cpu_address),
                routine_bytes_hex: hex_bytes(routine.expected),
            })
        })
        .collect()
}

pub(super) fn calculate_active_slot_ceiling(slots: &[SlotReport]) -> Result<ActiveSlotCeiling> {
    ensure!(
        slots.len() == TILES_PER_PAGE,
        "active slot ceiling requires one complete font page"
    );
    let confirmed_protected_code_count = slots
        .iter()
        .filter(|slot| slot.code_assignment == Decision::Protected)
        .count();
    ensure!(
        LAYOUT_RESERVED_CODES
            .iter()
            .all(|code| slots[usize::from(*code)].code_assignment == Decision::Unresolved),
        "provisional layout reservation overlaps a protected code"
    );
    let current_reserved_code_count = confirmed_protected_code_count + LAYOUT_RESERVED_CODES.len();

    let reported_protected_codes = slots
        .iter()
        .filter(|slot| slot.code_assignment == Decision::Protected)
        .map(|slot| slot.code)
        .collect::<BTreeSet<_>>();
    ensure!(
        reported_protected_codes == protected_original_codes(),
        "font report protected-code set disagrees with the shared slot contract"
    );

    Ok(ActiveSlotCeiling {
        total_font_code_count: TILES_PER_PAGE,
        confirmed_protected_code_count,
        provisional_layout_reserved_codes: LAYOUT_RESERVED_CODES.to_vec(),
        provisional_layout_reserved_codes_hex: LAYOUT_RESERVED_CODES
            .iter()
            .map(|code| format!("{code:02X}"))
            .collect(),
        current_reserved_code_count,
        current_hangul_slot_ceiling: TILES_PER_PAGE - current_reserved_code_count,
        proof_boundary: "ceiling_after_confirmed_original_and_composite_layout_reservations_not_a_final_per_screen_budget",
    })
}

pub(super) fn describe_mmc4_chr_writers(prg: &[u8]) -> Result<Vec<Mmc4ChrWriterReport>> {
    ensure!(
        prg.len() == PRG_SIZE,
        "unexpected PRG size for MMC4 writer inventory"
    );

    MMC4_CHR_WRITERS
        .iter()
        .map(|writer| {
            let prg_offset = fixed_bank_prg_offset(writer.cpu_address)?;
            let end = prg_offset + writer.expected.len();
            ensure!(
                prg[prg_offset..end] == writer.expected,
                "MMC4 CHR writer at ${:04X} changed",
                writer.cpu_address
            );

            Ok(Mmc4ChrWriterReport {
                cpu_address: writer.cpu_address,
                cpu_address_hex: format!("0x{:04X}", writer.cpu_address),
                shadow_address: writer.shadow_address,
                shadow_address_hex: format!("0x{:02X}", writer.shadow_address),
                page_group_shadow_address: 0x52,
                page_group_shadow_address_hex: "0x52".to_owned(),
                hardware_register: writer.hardware_register,
                hardware_register_hex: format!("0x{:04X}", writer.hardware_register),
                latch_domain: writer.latch_domain,
                routine_bytes_hex: hex_bytes(&writer.expected),
                direct_jsr_candidates: find_absolute_transfer_candidates(
                    prg,
                    writer.cpu_address,
                    0x20,
                ),
                direct_jmp_candidates: find_absolute_transfer_candidates(
                    prg,
                    writer.cpu_address,
                    0x4C,
                ),
            })
        })
        .collect()
}

pub(super) fn fixed_bank_prg_offset(cpu_address: u16) -> Result<usize> {
    ensure!(
        cpu_address >= 0xC000,
        "fixed-bank CPU address must be at or above $C000"
    );
    Ok(PRG_SIZE - PRG_BANK_SIZE + usize::from(cpu_address - 0xC000))
}

pub(super) fn validate_known_references(source: &[u8]) -> Result<()> {
    for reference in &KNOWN_REFERENCES {
        let end = reference
            .file_offset
            .checked_add(reference.expected.len())
            .context("known reference range overflow")?;
        ensure!(
            end <= source.len(),
            "known reference {} is outside the source image",
            reference.id
        );
        ensure!(
            source[reference.file_offset..end] == *reference.expected,
            "known reference {} bytes changed at {:#X}",
            reference.id,
            reference.file_offset
        );
    }
    Ok(())
}
