use crate::{
    battle_runtime_state::{BATTLE_RUNTIME_STATE, BattleRecipeDirectory},
    rp2a03::Instruction,
};

use super::{DIRECTORY_POINTER_HIGH, DIRECTORY_POINTER_LOW, runtime::RecipeDirectoryAddresses};

pub(super) use crate::battle_runtime_state::BattleRecipeIndexSource as RecipeIndexSource;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RuntimeRecipeField {
    pub(super) directory: u16,
    pub(super) index_source: RecipeIndexSource,
}

pub(super) fn runtime_recipe_fields(
    directories: RecipeDirectoryAddresses,
) -> [RuntimeRecipeField; 9] {
    BATTLE_RUNTIME_STATE
        .recipe_fields()
        .map(|field| RuntimeRecipeField {
            directory: match field.directory {
                BattleRecipeDirectory::Unit => directories.unit,
                BattleRecipeDirectory::Enemy => directories.enemy,
                BattleRecipeDirectory::Class => directories.class,
                BattleRecipeDirectory::Item => directories.item,
                BattleRecipeDirectory::Terrain => directories.terrain,
                BattleRecipeDirectory::Dialogue => directories.dialogue,
            },
            index_source: field.index_source,
        })
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
    fn runtime_directories_follow_the_shared_battle_recipe_roles() {
        let directories = RecipeDirectoryAddresses {
            unit: 0x8123,
            enemy: 0x8245,
            class: 0x8367,
            item: 0x8489,
            terrain: 0x85AB,
            dialogue: 0x86CD,
        };

        let fields = runtime_recipe_fields(directories);
        assert_eq!(
            fields.map(|field| field.directory),
            [
                directories.unit,
                directories.enemy,
                directories.class,
                directories.class,
                directories.item,
                directories.item,
                directories.terrain,
                directories.terrain,
                directories.dialogue,
            ]
        );
        assert_eq!(
            fields.map(|field| field.index_source),
            BATTLE_RUNTIME_STATE
                .recipe_fields()
                .map(|field| field.index_source)
        );
    }
}
