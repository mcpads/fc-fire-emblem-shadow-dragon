//! Supported-source battle state shared by analysis, code generation, and runtime verification.
//!
//! These values describe one source-game ABI.  Consumers must derive their field order,
//! selector projection, and composition lifetime from this module instead of repeating
//! addresses locally.

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum BattleRecipeDirectory {
    Unit,
    Enemy,
    Class,
    Item,
    Terrain,
    Dialogue,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum BattleRecipeIndexSource {
    UnitIdentity(u16),
    EnemyIdentity(u16),
    OneBasedIdentity(u16),
    DirectIndex(u16),
    ProjectedDialogue,
}

impl BattleRecipeIndexSource {
    #[cfg(test)]
    pub(crate) const fn staging_address(self) -> Option<u16> {
        match self {
            Self::UnitIdentity(address)
            | Self::EnemyIdentity(address)
            | Self::OneBasedIdentity(address)
            | Self::DirectIndex(address) => Some(address),
            Self::ProjectedDialogue => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct BattleRecipeField {
    pub(crate) directory: BattleRecipeDirectory,
    pub(crate) index_source: BattleRecipeIndexSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BattleDialogueSelectorProjection {
    pub(crate) observed_selector_address: u16,
    pub(crate) forced_selector: u8,
    pub(crate) required_nonzero_addresses: [u16; 3],
    pub(crate) required_zero_addresses: [u16; 1],
    pub(crate) dynamic_record_index_address: u16,
    pub(crate) dynamic_record_index_source_address: u16,
    pub(crate) dynamic_record_index_or_mask: u8,
    pub(crate) terminator_address: u16,
    pub(crate) terminator_value: u8,
}

impl BattleDialogueSelectorProjection {
    pub(crate) fn project<E>(
        self,
        observed_selector: u8,
        mut read_internal: impl FnMut(u16) -> Result<u8, E>,
    ) -> Result<(u8, bool), E> {
        let mut predicate_matched = true;
        for address in self.required_nonzero_addresses {
            predicate_matched &= read_internal(address)? != 0;
        }
        for address in self.required_zero_addresses {
            predicate_matched &= read_internal(address)? == 0;
        }
        Ok((
            if predicate_matched {
                self.forced_selector
            } else {
                observed_selector
            },
            predicate_matched,
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BattleRuntimeStateLayout {
    pub(crate) battle_record_addresses: [u16; 2],
    pub(crate) battle_record_byte_count: usize,
    pub(crate) live_record_identity_offset: u8,
    pub(crate) live_record_class_offset: u8,
    pub(crate) live_record_equipped_item_offset: u8,
    pub(crate) staged_participant_identity_addresses: [u16; 2],
    pub(crate) staged_class_identity_addresses: [u16; 2],
    pub(crate) staged_item_source_index_addresses: [u16; 2],
    pub(crate) staged_terrain_source_index_addresses: [u16; 2],
    pub(crate) staging_write_bounds: [u16; 2],
    pub(crate) shared_phase_address: u16,
    pub(crate) shared_phase_count: u8,
    pub(crate) active_flag_address: u16,
    pub(crate) dialogue_table_set_address: u16,
    pub(crate) dialogue_state_address: u16,
    pub(crate) dialogue_selector_projection: BattleDialogueSelectorProjection,
}

impl BattleRuntimeStateLayout {
    pub(crate) const fn recipe_fields(self) -> [BattleRecipeField; 9] {
        [
            BattleRecipeField {
                directory: BattleRecipeDirectory::Unit,
                index_source: BattleRecipeIndexSource::UnitIdentity(
                    self.staged_participant_identity_addresses[0],
                ),
            },
            BattleRecipeField {
                directory: BattleRecipeDirectory::Enemy,
                index_source: BattleRecipeIndexSource::EnemyIdentity(
                    self.staged_participant_identity_addresses[1],
                ),
            },
            BattleRecipeField {
                directory: BattleRecipeDirectory::Class,
                index_source: BattleRecipeIndexSource::OneBasedIdentity(
                    self.staged_class_identity_addresses[0],
                ),
            },
            BattleRecipeField {
                directory: BattleRecipeDirectory::Class,
                index_source: BattleRecipeIndexSource::OneBasedIdentity(
                    self.staged_class_identity_addresses[1],
                ),
            },
            BattleRecipeField {
                directory: BattleRecipeDirectory::Item,
                index_source: BattleRecipeIndexSource::DirectIndex(
                    self.staged_item_source_index_addresses[0],
                ),
            },
            BattleRecipeField {
                directory: BattleRecipeDirectory::Item,
                index_source: BattleRecipeIndexSource::DirectIndex(
                    self.staged_item_source_index_addresses[1],
                ),
            },
            BattleRecipeField {
                directory: BattleRecipeDirectory::Terrain,
                index_source: BattleRecipeIndexSource::DirectIndex(
                    self.staged_terrain_source_index_addresses[0],
                ),
            },
            BattleRecipeField {
                directory: BattleRecipeDirectory::Terrain,
                index_source: BattleRecipeIndexSource::DirectIndex(
                    self.staged_terrain_source_index_addresses[1],
                ),
            },
            BattleRecipeField {
                directory: BattleRecipeDirectory::Dialogue,
                index_source: BattleRecipeIndexSource::ProjectedDialogue,
            },
        ]
    }

    pub(crate) fn battle_record_ranges(self) -> [std::ops::RangeInclusive<u16>; 2] {
        self.battle_record_addresses
            .map(|start| start..=start + u16::try_from(self.battle_record_byte_count).unwrap() - 1)
    }

    pub(crate) fn staging_write_range(self) -> std::ops::RangeInclusive<u16> {
        self.staging_write_bounds[0]..=self.staging_write_bounds[1]
    }
}

pub(crate) const BATTLE_RUNTIME_STATE: BattleRuntimeStateLayout = BattleRuntimeStateLayout {
    battle_record_addresses: [0x76F4, 0x7715],
    battle_record_byte_count: 0x1B,
    live_record_identity_offset: 0x00,
    live_record_class_offset: 0x01,
    live_record_equipped_item_offset: 0x13,
    staged_participant_identity_addresses: [0x0304, 0x0305],
    staged_class_identity_addresses: [0x0306, 0x0307],
    staged_item_source_index_addresses: [0x0320, 0x0321],
    staged_terrain_source_index_addresses: [0x0322, 0x0323],
    staging_write_bounds: [0x0304, 0x0327],
    shared_phase_address: 0x047C,
    shared_phase_count: 0x20,
    active_flag_address: 0x047D,
    dialogue_table_set_address: 0x7935,
    dialogue_state_address: 0x7937,
    dialogue_selector_projection: BattleDialogueSelectorProjection {
        observed_selector_address: 0x7936,
        forced_selector: 0x3E,
        required_nonzero_addresses: [0x0334, 0x0479, 0x0335],
        required_zero_addresses: [0x05DF],
        dynamic_record_index_address: 0x7A4B,
        dynamic_record_index_source_address: 0x0479,
        dynamic_record_index_or_mask: 0x60,
        terminator_address: 0x7A4C,
        terminator_value: 0xEF,
    },
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct BattleSourceWrite {
    pub(crate) prg_bank: u8,
    pub(crate) cpu_address: u16,
}

/// Reactivates the next round inside an already composed battle.
pub(crate) const SAME_BATTLE_ROUND_ACTIVATION_WRITE: BattleSourceWrite = BattleSourceWrite {
    prg_bank: 0x05,
    cpu_address: 0x82BB,
};

/// Every supported-source site that stores literal `1` in the battle-active flag.
pub(crate) const BATTLE_ACTIVE_ONE_WRITES: [BattleSourceWrite; 4] = [
    SAME_BATTLE_ROUND_ACTIVATION_WRITE,
    BattleSourceWrite {
        prg_bank: 0x06,
        cpu_address: 0x9300,
    },
    BattleSourceWrite {
        prg_bank: 0x06,
        cpu_address: 0x9D52,
    },
    BattleSourceWrite {
        prg_bank: 0x07,
        cpu_address: 0xAC17,
    },
];

/// Source writes that start a new shared battle-text composition lifetime.
pub(crate) const BATTLE_COMPOSITION_LIFETIME_START_WRITES: [BattleSourceWrite; 3] = [
    BattleSourceWrite {
        prg_bank: 0x06,
        cpu_address: 0x9300,
    },
    BattleSourceWrite {
        prg_bank: 0x06,
        cpu_address: 0x9D52,
    },
    BattleSourceWrite {
        prg_bank: 0x07,
        cpu_address: 0xAC17,
    },
];

pub(crate) const SOUND_TEST_BATTLE_COMPOSITION_LIFETIME_START_WRITE: BattleSourceWrite =
    BattleSourceWrite {
        prg_bank: 0x07,
        cpu_address: 0xAC17,
    };

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_source_recipe_fields_follow_the_staging_layout() {
        let fields = BATTLE_RUNTIME_STATE.recipe_fields();

        assert_eq!(fields.len(), 9);
        assert_eq!(
            fields.map(|field| field.index_source.staging_address()),
            [
                Some(0x0304),
                Some(0x0305),
                Some(0x0306),
                Some(0x0307),
                Some(0x0320),
                Some(0x0321),
                Some(0x0322),
                Some(0x0323),
                None,
            ]
        );
        assert!(
            fields
                .iter()
                .filter_map(|field| field.index_source.staging_address())
                .all(|address| BATTLE_RUNTIME_STATE
                    .staging_write_range()
                    .contains(&address))
        );
        assert_eq!(
            BATTLE_RUNTIME_STATE.battle_record_ranges(),
            [0x76F4..=0x770E, 0x7715..=0x772F]
        );
    }

    #[test]
    fn same_battle_reactivation_is_not_a_new_composition_lifetime() {
        assert!(BATTLE_ACTIVE_ONE_WRITES.contains(&SAME_BATTLE_ROUND_ACTIVATION_WRITE));
        assert!(
            !BATTLE_COMPOSITION_LIFETIME_START_WRITES.contains(&SAME_BATTLE_ROUND_ACTIVATION_WRITE)
        );
        assert!(
            BATTLE_COMPOSITION_LIFETIME_START_WRITES
                .contains(&SOUND_TEST_BATTLE_COMPOSITION_LIFETIME_START_WRITE)
        );
    }

    #[test]
    fn selector_projection_uses_one_shared_predicate() {
        let selector = BATTLE_RUNTIME_STATE.dialogue_selector_projection;
        let (projected, matched) = selector
            .project(7, |address| {
                Ok::<_, ()>(if selector.required_nonzero_addresses.contains(&address) {
                    1
                } else {
                    0
                })
            })
            .unwrap();

        assert_eq!(projected, selector.forced_selector);
        assert!(matched);
    }
}
