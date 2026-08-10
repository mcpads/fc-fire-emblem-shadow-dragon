use std::collections::BTreeSet;

use anyhow::{Context, Result, ensure};

use crate::rom::{HEADER_SIZE, Rom};

use super::{
    CLASS_SOURCE_ENTRY_COUNT, ITEM_ELIGIBILITY_PRG_BANK, PRG_BANK_SIZE, SWITCHABLE_CPU_START,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct PlayerLoadoutCandidate {
    pub(super) required_identity: u8,
    pub(super) class_id: u8,
    pub(super) item_id: u8,
}

pub(super) fn bank_six_slice<'a>(
    rom: &'a Rom,
    cpu_address: u16,
    byte_count: usize,
    role: &str,
) -> Result<&'a [u8]> {
    ensure!(
        cpu_address >= SWITCHABLE_CPU_START,
        "{role} starts below the switchable PRG window"
    );
    let file_offset = HEADER_SIZE
        + usize::from(ITEM_ELIGIBILITY_PRG_BANK) * PRG_BANK_SIZE
        + usize::from(cpu_address - SWITCHABLE_CPU_START);
    rom.data()
        .get(file_offset..file_offset + byte_count)
        .with_context(|| format!("{role} is outside PRG bank 06"))
}

pub(super) fn item_family_class_lists(rom: &Rom, pointer_bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    pointer_bytes
        .chunks_exact(2)
        .enumerate()
        .map(|(family_index, bytes)| {
            let pointer = u16::from_le_bytes([bytes[0], bytes[1]]);
            ensure!(
                (0x8000..0xC000).contains(&pointer),
                "item family {family_index} class-list pointer is outside PRG bank 06"
            );
            let remaining = bank_six_slice(
                rom,
                pointer,
                usize::from(0xC000 - pointer),
                "item family class list",
            )?;
            let terminator = remaining
                .iter()
                .position(|byte| *byte == 0xEF)
                .with_context(|| {
                    format!("item family {family_index} class list has no terminator")
                })?;
            let classes = remaining[..terminator].to_vec();
            ensure!(
                !classes.is_empty(),
                "item family {family_index} has no classes"
            );
            ensure!(
                classes
                    .iter()
                    .all(|class_id| (1..=CLASS_SOURCE_ENTRY_COUNT as u8).contains(class_id)),
                "item family {family_index} has an out-of-range class identity"
            );
            Ok(classes)
        })
        .collect()
}

pub(super) fn eligible_player_loadouts(
    flags: &[u8],
    requirements: &[u8],
    family_thresholds: &[u8],
    family_class_lists: &[Vec<u8>],
) -> Result<BTreeSet<PlayerLoadoutCandidate>> {
    ensure!(
        !flags.is_empty() && flags.len() == requirements.len(),
        "class-item eligibility flag and requirement tables differ in size"
    );
    ensure!(
        family_thresholds.len() == family_class_lists.len(),
        "item family thresholds and class lists differ in count"
    );
    let mut loadouts = BTreeSet::new();
    for (source_index, (flags, requirement)) in flags.iter().zip(requirements).enumerate() {
        let item_id = u8::try_from(source_index + 1).context("item identity overflow")?;
        if flags & 0x01 != 0 {
            continue;
        }
        if requirement & 0x80 != 0 {
            for class_id in 1..=CLASS_SOURCE_ENTRY_COUNT as u8 {
                loadouts.insert(PlayerLoadoutCandidate {
                    required_identity: requirement & 0x7F,
                    class_id,
                    item_id,
                });
            }
            continue;
        }
        if let Some(family_index) = family_thresholds
            .iter()
            .position(|upper_bound| item_id < *upper_bound)
        {
            for class_id in &family_class_lists[family_index] {
                loadouts.insert(PlayerLoadoutCandidate {
                    required_identity: 0,
                    class_id: *class_id,
                    item_id,
                });
            }
        }
    }
    loadouts.insert(PlayerLoadoutCandidate {
        required_identity: 0,
        class_id: 0x16,
        item_id: 0x09,
    });
    Ok(loadouts)
}

pub(in crate::mapper165::battle_codebook_plan) fn battle_item_source_index(
    item_id: u8,
) -> Result<usize> {
    ensure!(item_id != 0, "battle item identity is zero");
    Ok(if item_id < 0x40 {
        usize::from(item_id - 1)
    } else {
        0x44
    })
}

pub(super) fn equip_candidate_source_indices(flags: &[u8]) -> Result<Vec<usize>> {
    let source_indices = flags
        .iter()
        .enumerate()
        .filter(|(_, flags)| *flags & 0x01 == 0)
        .map(|(source_index, _)| {
            let item_id = u8::try_from(source_index + 1).context("item identity overflow")?;
            battle_item_source_index(item_id)
        })
        .collect::<Result<BTreeSet<_>>>()?;
    Ok(source_indices.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_loadouts_follow_family_lists_and_preserve_required_identity() {
        let flags = [0; 4];
        let requirements = [0, 0x81, 0, 0];
        let loadouts =
            eligible_player_loadouts(&flags, &requirements, &[3, 5], &[vec![1], vec![2]]).unwrap();

        assert!(loadouts.contains(&PlayerLoadoutCandidate {
            required_identity: 0,
            class_id: 1,
            item_id: 1,
        }));
        assert!(loadouts.contains(&PlayerLoadoutCandidate {
            required_identity: 1,
            class_id: 23,
            item_id: 2,
        }));
        assert!(loadouts.contains(&PlayerLoadoutCandidate {
            required_identity: 0,
            class_id: 2,
            item_id: 4,
        }));
        assert!(!loadouts.contains(&PlayerLoadoutCandidate {
            required_identity: 0,
            class_id: 1,
            item_id: 4,
        }));
    }

    #[test]
    fn battle_item_projection_collapses_high_identities_to_the_breath_name() {
        assert_eq!(battle_item_source_index(1).unwrap(), 0);
        assert_eq!(battle_item_source_index(0x3F).unwrap(), 0x3E);
        assert_eq!(battle_item_source_index(0x40).unwrap(), 0x44);
        assert_eq!(battle_item_source_index(0x5B).unwrap(), 0x44);
        assert!(battle_item_source_index(0).is_err());
    }
}
