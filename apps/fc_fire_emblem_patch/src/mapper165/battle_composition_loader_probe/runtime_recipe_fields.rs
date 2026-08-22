use crate::rp2a03::Instruction;

use super::{DIRECTORY_POINTER_HIGH, DIRECTORY_POINTER_LOW, runtime::RecipeDirectoryAddresses};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RecipeIndexSource {
    UnitIdentity(u16),
    EnemyIdentity(u16),
    OneBasedIdentity(u16),
    DirectIndex(u16),
    ProjectedDialogue,
}

impl RecipeIndexSource {
    #[cfg(test)]
    pub(super) fn staging_address(self) -> Option<u16> {
        match self {
            Self::UnitIdentity(address)
            | Self::EnemyIdentity(address)
            | Self::OneBasedIdentity(address)
            | Self::DirectIndex(address) => Some(address),
            Self::ProjectedDialogue => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RuntimeRecipeField {
    pub(super) directory: u16,
    pub(super) index_source: RecipeIndexSource,
}

pub(super) fn runtime_recipe_fields(
    directories: RecipeDirectoryAddresses,
) -> [RuntimeRecipeField; 9] {
    [
        RuntimeRecipeField {
            directory: directories.unit,
            index_source: RecipeIndexSource::UnitIdentity(0x0304),
        },
        RuntimeRecipeField {
            directory: directories.enemy,
            index_source: RecipeIndexSource::EnemyIdentity(0x0305),
        },
        RuntimeRecipeField {
            directory: directories.class,
            index_source: RecipeIndexSource::OneBasedIdentity(0x0306),
        },
        RuntimeRecipeField {
            directory: directories.class,
            index_source: RecipeIndexSource::OneBasedIdentity(0x0307),
        },
        RuntimeRecipeField {
            directory: directories.item,
            index_source: RecipeIndexSource::DirectIndex(0x0320),
        },
        RuntimeRecipeField {
            directory: directories.item,
            index_source: RecipeIndexSource::DirectIndex(0x0321),
        },
        RuntimeRecipeField {
            directory: directories.terrain,
            index_source: RecipeIndexSource::DirectIndex(0x0322),
        },
        RuntimeRecipeField {
            directory: directories.terrain,
            index_source: RecipeIndexSource::DirectIndex(0x0323),
        },
        RuntimeRecipeField {
            directory: directories.dialogue,
            index_source: RecipeIndexSource::ProjectedDialogue,
        },
    ]
}

pub(super) fn select_recipe_directory(
    instructions: &mut Vec<Instruction>,
    selected_directory: &mut Option<u16>,
    directory: u16,
) {
    if *selected_directory == Some(directory) {
        return;
    }
    instructions.extend([
        Instruction::LdaImmediate(directory as u8),
        Instruction::StaZeroPage(DIRECTORY_POINTER_LOW),
        Instruction::LdaImmediate((directory >> 8) as u8),
        Instruction::StaZeroPage(DIRECTORY_POINTER_HIGH),
    ]);
    *selected_directory = Some(directory);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battle_source_staging_abi_maps_each_role_to_its_recipe_directory() {
        let directories = RecipeDirectoryAddresses {
            unit: 0x8123,
            enemy: 0x8245,
            class: 0x8367,
            item: 0x8489,
            terrain: 0x85AB,
            dialogue: 0x86CD,
        };

        assert_eq!(
            runtime_recipe_fields(directories),
            [
                RuntimeRecipeField {
                    directory: directories.unit,
                    index_source: RecipeIndexSource::UnitIdentity(0x0304),
                },
                RuntimeRecipeField {
                    directory: directories.enemy,
                    index_source: RecipeIndexSource::EnemyIdentity(0x0305),
                },
                RuntimeRecipeField {
                    directory: directories.class,
                    index_source: RecipeIndexSource::OneBasedIdentity(0x0306),
                },
                RuntimeRecipeField {
                    directory: directories.class,
                    index_source: RecipeIndexSource::OneBasedIdentity(0x0307),
                },
                RuntimeRecipeField {
                    directory: directories.item,
                    index_source: RecipeIndexSource::DirectIndex(0x0320),
                },
                RuntimeRecipeField {
                    directory: directories.item,
                    index_source: RecipeIndexSource::DirectIndex(0x0321),
                },
                RuntimeRecipeField {
                    directory: directories.terrain,
                    index_source: RecipeIndexSource::DirectIndex(0x0322),
                },
                RuntimeRecipeField {
                    directory: directories.terrain,
                    index_source: RecipeIndexSource::DirectIndex(0x0323),
                },
                RuntimeRecipeField {
                    directory: directories.dialogue,
                    index_source: RecipeIndexSource::ProjectedDialogue,
                },
            ]
        );
    }
}
