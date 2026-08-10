use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};

use crate::sha1_hex;

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
}
