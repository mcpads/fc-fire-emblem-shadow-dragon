use anyhow::{Result, ensure};

pub(crate) const SOURCE_PPU_QUEUE_START: u16 = 0x0781;
pub(crate) const SOURCE_PPU_QUEUE_END: u16 = 0x07DF;

pub(crate) const BATTLE_REMAP_PAIR_TABLE_START: u16 = 0x07E0;
pub(crate) const BATTLE_REMAP_PAIR_TABLE_END: u16 = 0x07EF;

pub(crate) const DIALOGUE_RUNTIME_STORAGE_START: u16 = 0x07F0;
pub(crate) const DIALOGUE_RUNTIME_STORAGE_END: u16 = 0x07FD;

pub(crate) const BATTLE_REMAP_STATE_ADDRESS: u16 = 0x07FE;
pub(crate) const BATTLE_DIALOGUE_CACHE_KEY_ADDRESS: u16 = 0x07FF;

pub(crate) const PROVEN_RUNTIME_STORAGE_START: u16 = DIALOGUE_RUNTIME_STORAGE_START;
pub(crate) const PROVEN_RUNTIME_STORAGE_END: u16 = BATTLE_DIALOGUE_CACHE_KEY_ADDRESS;

#[derive(Clone, Copy)]
struct OwnedRange {
    role: &'static str,
    start: u16,
    end: u16,
}

pub(crate) fn bind_integrated_runtime_storage_layout() -> Result<()> {
    validate_owned_ranges(&[
        OwnedRange {
            role: "source PPU command queue",
            start: SOURCE_PPU_QUEUE_START,
            end: SOURCE_PPU_QUEUE_END,
        },
        OwnedRange {
            role: "battle remap pair table",
            start: BATTLE_REMAP_PAIR_TABLE_START,
            end: BATTLE_REMAP_PAIR_TABLE_END,
        },
        OwnedRange {
            role: "dialogue and composite runtime state",
            start: DIALOGUE_RUNTIME_STORAGE_START,
            end: DIALOGUE_RUNTIME_STORAGE_END,
        },
        OwnedRange {
            role: "battle remap state",
            start: BATTLE_REMAP_STATE_ADDRESS,
            end: BATTLE_REMAP_STATE_ADDRESS,
        },
        OwnedRange {
            role: "battle dialogue cache key",
            start: BATTLE_DIALOGUE_CACHE_KEY_ADDRESS,
            end: BATTLE_DIALOGUE_CACHE_KEY_ADDRESS,
        },
    ])
}

fn validate_owned_ranges(ranges: &[OwnedRange]) -> Result<()> {
    ensure!(!ranges.is_empty(), "runtime storage ownership is empty");
    for range in ranges {
        ensure!(
            range.start <= range.end && range.end <= 0x07FF,
            "{} has an invalid internal-RAM range {:04X}..{:04X}",
            range.role,
            range.start,
            range.end
        );
    }

    let mut ordered = ranges.to_vec();
    ordered.sort_by_key(|range| (range.start, range.end));
    for pair in ordered.windows(2) {
        ensure!(
            pair[0].end < pair[1].start,
            "runtime storage owners overlap: {} {:04X}..{:04X} and {} {:04X}..{:04X}",
            pair[0].role,
            pair[0].start,
            pair[0].end,
            pair[1].role,
            pair[1].start,
            pair[1].end
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrated_runtime_owners_fill_the_queue_tail_without_overlap() {
        bind_integrated_runtime_storage_layout().unwrap();
        assert_eq!(SOURCE_PPU_QUEUE_END + 1, BATTLE_REMAP_PAIR_TABLE_START);
        assert_eq!(
            BATTLE_REMAP_PAIR_TABLE_END + 1,
            DIALOGUE_RUNTIME_STORAGE_START
        );
        assert_eq!(DIALOGUE_RUNTIME_STORAGE_END + 1, BATTLE_REMAP_STATE_ADDRESS);
        assert_eq!(
            BATTLE_REMAP_STATE_ADDRESS + 1,
            BATTLE_DIALOGUE_CACHE_KEY_ADDRESS
        );
        assert_eq!(BATTLE_DIALOGUE_CACHE_KEY_ADDRESS, 0x07FF);
    }

    #[test]
    fn the_previous_queue_and_remap_state_collision_is_rejected() {
        let error = validate_owned_ranges(&[
            OwnedRange {
                role: "source PPU command queue",
                start: SOURCE_PPU_QUEUE_START,
                end: SOURCE_PPU_QUEUE_END,
            },
            OwnedRange {
                role: "old battle remap state",
                start: 0x07DF,
                end: 0x07DF,
            },
        ])
        .unwrap_err();

        assert!(error.to_string().contains("overlap"));
    }

    #[test]
    fn an_owner_outside_internal_ram_is_rejected() {
        let error = validate_owned_ranges(&[OwnedRange {
            role: "overflowing owner",
            start: 0x07FF,
            end: 0x0800,
        }])
        .unwrap_err();

        assert!(error.to_string().contains("invalid internal-RAM range"));
    }
}
