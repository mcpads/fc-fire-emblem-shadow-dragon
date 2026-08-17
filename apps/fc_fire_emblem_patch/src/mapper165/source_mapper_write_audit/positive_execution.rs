use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

use crate::{mapper165::executable_mapper_writes::MappedPrgLocation, rom::Rom};

use super::{FIXED_PRG_BANK, source_mapped_location};

const BATTLE_PHASE_GRAPH: &str = "battle_phase_catalog";
const DIALOGUE_INTERRUPT_AUDIO_GRAPH: &str = "main_dialogue_nmi_and_audio_positive_graph";

/// Positive source execution slices already bound by their owning battle and dialogue contracts.
/// This is deliberately not a complete executable-root ledger.
pub(super) struct SourcePositiveExecutionGraph {
    instruction_roles: BTreeMap<(u8, u16), BTreeSet<&'static str>>,
    indirect_write_sites_below_mapper_space: BTreeSet<(u8, u16, u8)>,
}

impl SourcePositiveExecutionGraph {
    pub(super) fn instruction_starts(&self) -> impl Iterator<Item = (u8, u16)> + '_ {
        self.instruction_roles.keys().copied()
    }

    pub(super) fn instruction_count(&self) -> usize {
        self.instruction_roles.len()
    }

    pub(super) fn roles_at(&self, bank: u8, address: u16) -> Option<&BTreeSet<&'static str>> {
        self.instruction_roles.get(&(bank, address))
    }

    pub(super) fn indirect_write_sites_below_mapper_space(&self) -> &BTreeSet<(u8, u16, u8)> {
        &self.indirect_write_sites_below_mapper_space
    }

    pub(super) fn mapped_instruction_starts(&self) -> Result<BTreeSet<MappedPrgLocation>> {
        self.instruction_starts()
            .map(|(bank, address)| source_mapped_location(bank, address))
            .collect()
    }
}

pub(super) fn bind_source_positive_execution_graph(
    source: &Rom,
) -> Result<SourcePositiveExecutionGraph> {
    let battle =
        crate::mapper165::battle_codebook_plan::phase_cooccurrence::battle_phase_reachable_instruction_starts(
            source,
        )?;
    let battle_indirect =
        crate::mapper165::battle_codebook_plan::bind_indirect_write_sites_below_mapper_space(
            source,
        )?;
    let dialogue_interrupt_audio =
        crate::full_translation_install::bind_dialogue_interrupt_audio_mapper_write_slice(source)?;

    let mut instruction_roles = BTreeMap::<_, BTreeSet<_>>::new();
    for (role, starts) in [
        (BATTLE_PHASE_GRAPH, &battle),
        (
            DIALOGUE_INTERRUPT_AUDIO_GRAPH,
            &dialogue_interrupt_audio.reachable_instruction_starts,
        ),
    ] {
        for &(bank, address) in starts {
            let location = normalize_source_location(bank, address)?;
            instruction_roles.entry(location).or_default().insert(role);
        }
    }
    ensure!(
        !instruction_roles.is_empty(),
        "source positive execution graph contains no instructions"
    );

    // These source contracts already report the physical PRG bank used to bind each instruction.
    // Preserve that identity instead of reinterpreting it as the caller's switchable-bank context.
    let indirect_write_sites_below_mapper_space = battle_indirect
        .union(&dialogue_interrupt_audio.indirect_write_sites_below_mapper_space)
        .copied()
        .collect();

    Ok(SourcePositiveExecutionGraph {
        instruction_roles,
        indirect_write_sites_below_mapper_space,
    })
}

fn normalize_source_location(bank: u8, address: u16) -> Result<(u8, u16)> {
    ensure!(
        address >= 0x8000,
        "source positive execution graph escaped PRG space at {bank:02X}:${address:04X}"
    );
    Ok((
        if address >= 0xC000 {
            FIXED_PRG_BANK
        } else {
            bank
        },
        address,
    ))
}
