use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};

use crate::sha1_hex;

use super::BattleRuntimeRecipeInput;

const MAGIC: &[u8; 4] = b"FBRC";
const FORMAT: u8 = 1;
const HEADER_BYTE_COUNT: usize = 32;
const UNIT_DIRECTORY_COUNT: usize = 52;
const ENEMY_DIRECTORY_COUNT: usize = 69;
const CLASS_DIRECTORY_COUNT: usize = 24;
const ITEM_DIRECTORY_COUNT: usize = 91;
const TERRAIN_DIRECTORY_COUNT: usize = 16;
const DIALOGUE_DIRECTORY_COUNT: usize = 65;
const MISSING_RECIPE_OFFSET: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub(super) enum RecipeRole {
    Common = 0,
    UnitName = 1,
    EnemyName = 2,
    Class = 3,
    Item = 4,
    Terrain = 5,
    Dialogue = 6,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct RecipePair {
    pub(super) color: u8,
    pub(super) atlas_index: u16,
}

#[derive(Default)]
pub(super) struct RecipeCatalog {
    recipes: BTreeMap<(RecipeRole, usize), Vec<RecipePair>>,
    dialogue_aliases: BTreeMap<usize, usize>,
    glyph_reference_count: usize,
    maximum_glyph_count: usize,
}

pub(super) struct EncodedRecipeCatalog {
    pub(super) bytes: Vec<u8>,
    pub(super) logical_recipe_count: usize,
    pub(super) unique_payload_count: usize,
    pub(super) glyph_reference_count: usize,
    pub(super) maximum_glyph_count: usize,
    pub(super) missing_directory_entry_count: usize,
    pub(super) sha1: String,
}

pub(super) struct EncodedRuntimeRecipeSelection {
    pub(super) recipe_offsets: Vec<u16>,
    pub(super) overlays: Vec<RecipePair>,
    pub(super) glyph_reference_count: usize,
}

impl RecipeCatalog {
    pub(super) fn add(
        &mut self,
        role: RecipeRole,
        source_index: usize,
        pairs: Vec<RecipePair>,
    ) -> Result<()> {
        ensure!(
            self.recipes
                .insert((role, source_index), pairs.clone())
                .is_none(),
            "duplicate battle recipe role {} source {source_index}",
            role as u8
        );
        self.glyph_reference_count += pairs.len();
        self.maximum_glyph_count = self.maximum_glyph_count.max(pairs.len());
        Ok(())
    }

    pub(super) fn add_dialogue_alias(
        &mut self,
        selector: usize,
        canonical_selector: usize,
    ) -> Result<()> {
        ensure!(
            selector < DIALOGUE_DIRECTORY_COUNT,
            "battle dialogue selector {selector} exceeds the recipe directory"
        );
        ensure!(
            self.dialogue_aliases
                .insert(selector, canonical_selector)
                .is_none(),
            "duplicate battle dialogue recipe selector {selector}"
        );
        Ok(())
    }

    pub(super) fn encode(
        &self,
        abstract_color_count: usize,
        atlas_tile_count: usize,
    ) -> Result<EncodedRecipeCatalog> {
        ensure!(
            abstract_color_count <= usize::from(u8::MAX),
            "battle recipe color count exceeds the blob header"
        );
        ensure!(
            atlas_tile_count <= usize::from(u16::MAX),
            "battle recipe atlas count exceeds the blob header"
        );
        ensure!(
            self.recipes.contains_key(&(RecipeRole::Common, 0)),
            "battle recipe catalog has no common recipe"
        );
        ensure!(
            self.dialogue_aliases.len() == DIALOGUE_DIRECTORY_COUNT,
            "battle recipe catalog does not cover every dialogue selector"
        );
        for ((role, source_index), pairs) in &self.recipes {
            for pair in pairs {
                ensure!(
                    usize::from(pair.color) < abstract_color_count,
                    "battle recipe role {} source {source_index} color {} exceeds the declared count {abstract_color_count}",
                    *role as u8,
                    pair.color
                );
                ensure!(
                    usize::from(pair.atlas_index) < atlas_tile_count,
                    "battle recipe role {} source {source_index} atlas index {} exceeds the declared count {atlas_tile_count}",
                    *role as u8,
                    pair.atlas_index
                );
            }
        }
        let directory_layout = DirectoryLayout::new()?;
        let mut bytes = vec![0; directory_layout.payload_offset];
        for offset in directory_layout.all_directory_offsets() {
            write_u16(&mut bytes, offset, MISSING_RECIPE_OFFSET)?;
        }

        let mut recipe_offsets = BTreeMap::new();
        let mut payload_offsets = BTreeMap::<Vec<RecipePair>, u16>::new();
        for (key, pairs) in &self.recipes {
            let offset = if let Some(offset) = payload_offsets.get(pairs) {
                *offset
            } else {
                let offset = u16::try_from(bytes.len())
                    .context("battle recipe blob exceeds 16-bit offsets")?;
                bytes.push(
                    u8::try_from(pairs.len())
                        .context("battle recipe exceeds one-byte glyph count")?,
                );
                for pair in pairs {
                    bytes.push(pair.color);
                    bytes.extend_from_slice(&pair.atlas_index.to_le_bytes());
                }
                payload_offsets.insert(pairs.clone(), offset);
                offset
            };
            recipe_offsets.insert(*key, offset);
            if key.0 != RecipeRole::Dialogue {
                directory_layout.write_recipe_offset(&mut bytes, key.0, key.1, offset)?;
            }
        }
        for (selector, canonical_selector) in &self.dialogue_aliases {
            let offset = recipe_offsets
                .get(&(RecipeRole::Dialogue, *canonical_selector))
                .copied()
                .with_context(|| {
                    format!(
                        "battle dialogue selector {selector} references missing canonical recipe {canonical_selector}"
                    )
                })?;
            directory_layout.write_recipe_offset(
                &mut bytes,
                RecipeRole::Dialogue,
                *selector,
                offset,
            )?;
        }

        let common_offset = recipe_offsets[&(RecipeRole::Common, 0)];
        bytes[0..4].copy_from_slice(MAGIC);
        bytes[4] = FORMAT;
        bytes[5] = u8::try_from(abstract_color_count)
            .context("battle abstract color count exceeds blob header")?;
        write_u16(
            &mut bytes,
            6,
            u16::try_from(atlas_tile_count)
                .context("battle atlas tile count exceeds blob header")?,
        )?;
        let total_byte_count =
            u16::try_from(bytes.len()).context("battle recipe blob exceeds header size field")?;
        write_u16(&mut bytes, 8, total_byte_count)?;
        write_u16(
            &mut bytes,
            10,
            u16::try_from(self.recipes.len()).context("battle recipe count exceeds blob header")?,
        )?;
        write_u16(
            &mut bytes,
            12,
            u16::try_from(payload_offsets.len())
                .context("battle recipe payload count exceeds blob header")?,
        )?;
        write_u16(&mut bytes, 14, common_offset)?;
        write_u16(&mut bytes, 16, u16::try_from(directory_layout.unit)?)?;
        write_u16(&mut bytes, 18, u16::try_from(directory_layout.enemy)?)?;
        write_u16(&mut bytes, 20, u16::try_from(directory_layout.class)?)?;
        write_u16(&mut bytes, 22, u16::try_from(directory_layout.item)?)?;
        write_u16(&mut bytes, 24, u16::try_from(directory_layout.terrain)?)?;
        write_u16(&mut bytes, 26, u16::try_from(directory_layout.dialogue)?)?;
        write_u16(
            &mut bytes,
            28,
            u16::try_from(directory_layout.payload_offset)?,
        )?;
        bytes[30] = 3;
        bytes[31] = 0;

        let missing_directory_entry_count = directory_layout
            .all_directory_offsets()
            .filter(|offset| read_u16(&bytes, *offset) == MISSING_RECIPE_OFFSET)
            .count();
        let sha1 = sha1_hex(&bytes);
        Ok(EncodedRecipeCatalog {
            bytes,
            logical_recipe_count: self.recipes.len(),
            unique_payload_count: payload_offsets.len(),
            glyph_reference_count: self.glyph_reference_count,
            maximum_glyph_count: self.maximum_glyph_count,
            missing_directory_entry_count,
            sha1,
        })
    }
}

pub(super) fn select_runtime_recipes(
    bytes: &[u8],
    input: BattleRuntimeRecipeInput,
) -> Result<EncodedRuntimeRecipeSelection> {
    let layout = validate_blob(bytes)?;
    let mut recipe_offsets = Vec::with_capacity(10);
    recipe_offsets.push(read_u16(bytes, 14));
    for identity in input.participant_record_identities {
        let source_index = usize::from(
            (identity & 0x7F)
                .checked_sub(1)
                .context("battle participant recipe identity is zero")?,
        );
        let role = if identity & 0x80 == 0 {
            RecipeRole::UnitName
        } else {
            RecipeRole::EnemyName
        };
        recipe_offsets.push(layout.read_recipe_offset(bytes, role, source_index)?);
    }
    for identity in input.class_record_identities {
        let source_index = usize::from(
            identity
                .checked_sub(1)
                .context("battle class recipe identity is zero")?,
        );
        recipe_offsets.push(layout.read_recipe_offset(bytes, RecipeRole::Class, source_index)?);
    }
    for source_index in input.item_source_indices {
        recipe_offsets.push(layout.read_recipe_offset(
            bytes,
            RecipeRole::Item,
            usize::from(source_index),
        )?);
    }
    for source_index in input.terrain_source_indices {
        recipe_offsets.push(layout.read_recipe_offset(
            bytes,
            RecipeRole::Terrain,
            usize::from(source_index),
        )?);
    }
    recipe_offsets.push(layout.read_recipe_offset(
        bytes,
        RecipeRole::Dialogue,
        usize::from(input.dialogue_selector),
    )?);

    ensure!(
        recipe_offsets.len() == 10,
        "battle runtime selection lost a recipe family"
    );
    let abstract_color_count = bytes[5];
    let atlas_tile_count = read_u16(bytes, 6);
    let mut color_atlas_indices = BTreeMap::<u8, u16>::new();
    let mut glyph_reference_count = 0;
    for recipe_offset in &recipe_offsets {
        ensure!(
            *recipe_offset != MISSING_RECIPE_OFFSET,
            "battle runtime input selects a missing recipe"
        );
        let offset = usize::from(*recipe_offset);
        ensure!(
            offset >= layout.payload_offset && offset < bytes.len(),
            "battle runtime recipe offset is outside the payload region"
        );
        let pair_count = usize::from(bytes[offset]);
        glyph_reference_count += pair_count;
        let end = offset
            .checked_add(1 + pair_count * 3)
            .context("battle runtime recipe payload range overflow")?;
        ensure!(
            end <= bytes.len(),
            "battle runtime recipe payload is truncated"
        );
        for pair_bytes in bytes[offset + 1..end].chunks_exact(3) {
            let pair = RecipePair {
                color: pair_bytes[0],
                atlas_index: u16::from_le_bytes([pair_bytes[1], pair_bytes[2]]),
            };
            ensure!(
                pair.color < abstract_color_count,
                "battle runtime recipe color exceeds the blob header"
            );
            ensure!(
                pair.atlas_index < atlas_tile_count,
                "battle runtime recipe atlas index exceeds the blob header"
            );
            if let Some(previous) = color_atlas_indices.insert(pair.color, pair.atlas_index) {
                ensure!(
                    previous == pair.atlas_index,
                    "battle runtime recipes assign two glyphs to abstract color {}",
                    pair.color
                );
            }
        }
    }
    Ok(EncodedRuntimeRecipeSelection {
        recipe_offsets,
        overlays: color_atlas_indices
            .into_iter()
            .map(|(color, atlas_index)| RecipePair { color, atlas_index })
            .collect(),
        glyph_reference_count,
    })
}

pub(super) fn has_directory_recipe(
    bytes: &[u8],
    role: RecipeRole,
    source_index: usize,
) -> Result<bool> {
    let layout = validate_blob(bytes)?;
    Ok(layout.read_recipe_offset(bytes, role, source_index)? != MISSING_RECIPE_OFFSET)
}

fn validate_blob(bytes: &[u8]) -> Result<DirectoryLayout> {
    ensure!(
        bytes.len() >= HEADER_BYTE_COUNT,
        "battle recipe blob is shorter than its header"
    );
    ensure!(&bytes[..4] == MAGIC, "battle recipe blob magic changed");
    ensure!(bytes[4] == FORMAT, "battle recipe blob format changed");
    ensure!(
        usize::from(read_u16(bytes, 8)) == bytes.len(),
        "battle recipe blob total byte count changed"
    );
    ensure!(bytes[30] == 3, "battle recipe pair stride changed");
    let layout = DirectoryLayout::new()?;
    for (header_offset, actual) in [
        (16, layout.unit),
        (18, layout.enemy),
        (20, layout.class),
        (22, layout.item),
        (24, layout.terrain),
        (26, layout.dialogue),
        (28, layout.payload_offset),
    ] {
        ensure!(
            usize::from(read_u16(bytes, header_offset)) == actual,
            "battle recipe blob directory layout changed"
        );
    }
    let common_offset = usize::from(read_u16(bytes, 14));
    ensure!(
        common_offset >= layout.payload_offset && common_offset < bytes.len(),
        "battle common recipe offset is outside the payload region"
    );
    Ok(layout)
}

struct DirectoryLayout {
    unit: usize,
    enemy: usize,
    class: usize,
    item: usize,
    terrain: usize,
    dialogue: usize,
    payload_offset: usize,
}

impl DirectoryLayout {
    fn new() -> Result<Self> {
        let unit = HEADER_BYTE_COUNT;
        let enemy = directory_end(unit, UNIT_DIRECTORY_COUNT)?;
        let class = directory_end(enemy, ENEMY_DIRECTORY_COUNT)?;
        let item = directory_end(class, CLASS_DIRECTORY_COUNT)?;
        let terrain = directory_end(item, ITEM_DIRECTORY_COUNT)?;
        let dialogue = directory_end(terrain, TERRAIN_DIRECTORY_COUNT)?;
        let payload_offset = directory_end(dialogue, DIALOGUE_DIRECTORY_COUNT)?;
        Ok(Self {
            unit,
            enemy,
            class,
            item,
            terrain,
            dialogue,
            payload_offset,
        })
    }

    fn write_recipe_offset(
        &self,
        bytes: &mut [u8],
        role: RecipeRole,
        source_index: usize,
        recipe_offset: u16,
    ) -> Result<()> {
        let (start, count) = match role {
            RecipeRole::Common => return Ok(()),
            RecipeRole::UnitName => (self.unit, UNIT_DIRECTORY_COUNT),
            RecipeRole::EnemyName => (self.enemy, ENEMY_DIRECTORY_COUNT),
            RecipeRole::Class => (self.class, CLASS_DIRECTORY_COUNT),
            RecipeRole::Item => (self.item, ITEM_DIRECTORY_COUNT),
            RecipeRole::Terrain => (self.terrain, TERRAIN_DIRECTORY_COUNT),
            RecipeRole::Dialogue => (self.dialogue, DIALOGUE_DIRECTORY_COUNT),
        };
        ensure!(
            source_index < count,
            "battle recipe role {} source {source_index} exceeds directory count {count}",
            role as u8
        );
        write_u16(bytes, start + source_index * 2, recipe_offset)
    }

    fn read_recipe_offset(
        &self,
        bytes: &[u8],
        role: RecipeRole,
        source_index: usize,
    ) -> Result<u16> {
        let (start, count) = self.directory(role)?;
        ensure!(
            source_index < count,
            "battle recipe role {} source {source_index} exceeds directory count {count}",
            role as u8
        );
        Ok(read_u16(bytes, start + source_index * 2))
    }

    fn directory(&self, role: RecipeRole) -> Result<(usize, usize)> {
        Ok(match role {
            RecipeRole::Common => anyhow::bail!("the common recipe has no directory"),
            RecipeRole::UnitName => (self.unit, UNIT_DIRECTORY_COUNT),
            RecipeRole::EnemyName => (self.enemy, ENEMY_DIRECTORY_COUNT),
            RecipeRole::Class => (self.class, CLASS_DIRECTORY_COUNT),
            RecipeRole::Item => (self.item, ITEM_DIRECTORY_COUNT),
            RecipeRole::Terrain => (self.terrain, TERRAIN_DIRECTORY_COUNT),
            RecipeRole::Dialogue => (self.dialogue, DIALOGUE_DIRECTORY_COUNT),
        })
    }

    fn all_directory_offsets(&self) -> impl Iterator<Item = usize> + '_ {
        [
            (self.unit, UNIT_DIRECTORY_COUNT),
            (self.enemy, ENEMY_DIRECTORY_COUNT),
            (self.class, CLASS_DIRECTORY_COUNT),
            (self.item, ITEM_DIRECTORY_COUNT),
            (self.terrain, TERRAIN_DIRECTORY_COUNT),
            (self.dialogue, DIALOGUE_DIRECTORY_COUNT),
        ]
        .into_iter()
        .flat_map(|(start, count)| (0..count).map(move |index| start + index * 2))
    }
}

fn directory_end(start: usize, entry_count: usize) -> Result<usize> {
    start
        .checked_add(
            entry_count
                .checked_mul(2)
                .context("battle recipe directory size overflow")?,
        )
        .context("battle recipe directory range overflow")
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) -> Result<()> {
    bytes
        .get_mut(offset..offset + 2)
        .context("battle recipe header write is outside the blob")?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encoded_catalog_has_fixed_directories_and_shared_dialogue_payloads() {
        let mut catalog = RecipeCatalog::default();
        catalog
            .add(
                RecipeRole::Common,
                0,
                vec![RecipePair {
                    color: 3,
                    atlas_index: 7,
                }],
            )
            .unwrap();
        catalog
            .add(
                RecipeRole::UnitName,
                0,
                vec![RecipePair {
                    color: 4,
                    atlas_index: 8,
                }],
            )
            .unwrap();
        catalog
            .add(
                RecipeRole::Dialogue,
                0,
                vec![RecipePair {
                    color: 5,
                    atlas_index: 9,
                }],
            )
            .unwrap();
        for selector in 0..DIALOGUE_DIRECTORY_COUNT {
            catalog.add_dialogue_alias(selector, 0).unwrap();
        }

        let encoded = catalog.encode(6, 10).unwrap();
        let layout = DirectoryLayout::new().unwrap();

        assert_eq!(&encoded.bytes[..4], MAGIC);
        assert_eq!(encoded.bytes[4], FORMAT);
        assert_eq!(encoded.bytes[5], 6);
        assert_eq!(read_u16(&encoded.bytes, 6), 10);
        assert_eq!(
            usize::from(read_u16(&encoded.bytes, 8)),
            encoded.bytes.len()
        );
        assert_ne!(read_u16(&encoded.bytes, layout.unit), MISSING_RECIPE_OFFSET);
        assert_eq!(
            read_u16(&encoded.bytes, layout.enemy),
            MISSING_RECIPE_OFFSET
        );
        let dialogue_offset = read_u16(&encoded.bytes, layout.dialogue);
        assert!((1..DIALOGUE_DIRECTORY_COUNT).all(|index| {
            read_u16(&encoded.bytes, layout.dialogue + index * 2) == dialogue_offset
        }));
    }

    #[test]
    fn encoded_catalog_rejects_pairs_outside_declared_ranges() {
        let mut catalog = RecipeCatalog::default();
        catalog
            .add(
                RecipeRole::Common,
                0,
                vec![RecipePair {
                    color: 1,
                    atlas_index: 0,
                }],
            )
            .unwrap();
        catalog.add(RecipeRole::Dialogue, 0, Vec::new()).unwrap();
        for selector in 0..DIALOGUE_DIRECTORY_COUNT {
            catalog.add_dialogue_alias(selector, 0).unwrap();
        }

        assert!(catalog.encode(1, 1).is_err());
    }

    #[test]
    fn runtime_selection_resolves_all_ten_recipe_families() {
        let mut catalog = RecipeCatalog::default();
        for (role, source_index, color) in [
            (RecipeRole::Common, 0, 0),
            (RecipeRole::UnitName, 0, 1),
            (RecipeRole::EnemyName, 0, 2),
            (RecipeRole::Class, 0, 3),
            (RecipeRole::Item, 0, 4),
            (RecipeRole::Terrain, 0, 5),
            (RecipeRole::Dialogue, 0, 6),
        ] {
            catalog
                .add(
                    role,
                    source_index,
                    vec![RecipePair {
                        color,
                        atlas_index: u16::from(color),
                    }],
                )
                .unwrap();
        }
        for selector in 0..DIALOGUE_DIRECTORY_COUNT {
            catalog.add_dialogue_alias(selector, 0).unwrap();
        }
        let encoded = catalog.encode(7, 7).unwrap();
        let selection = select_runtime_recipes(
            &encoded.bytes,
            BattleRuntimeRecipeInput {
                participant_record_identities: [1, 0x81],
                class_record_identities: [1, 1],
                item_source_indices: [0, 0],
                terrain_source_indices: [0, 0],
                dialogue_selector: 0,
            },
        )
        .unwrap();

        assert_eq!(selection.recipe_offsets.len(), 10);
        assert_eq!(selection.overlays.len(), 7);
        assert_eq!(
            selection
                .overlays
                .iter()
                .map(|pair| pair.color)
                .collect::<Vec<_>>(),
            (0..7).collect::<Vec<_>>()
        );
    }
}
