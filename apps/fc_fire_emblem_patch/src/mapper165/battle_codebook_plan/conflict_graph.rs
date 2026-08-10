use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::sha1_hex;

pub(super) struct BattleGlyphFamilies {
    pub(super) base: BTreeSet<char>,
    pub(super) unit_names: Vec<BTreeSet<char>>,
    pub(super) enemy_names: Vec<BTreeSet<char>>,
    pub(super) classes: Vec<BTreeSet<char>>,
    pub(super) items: Vec<BTreeSet<char>>,
    pub(super) terrains: Vec<BTreeSet<char>>,
    pub(super) dialogue_records: Vec<BTreeSet<char>>,
}

pub(super) struct FamilyEntryCounts {
    pub(super) unit_names: usize,
    pub(super) enemy_names: usize,
    pub(super) classes: usize,
    pub(super) items: usize,
    pub(super) terrains: usize,
    pub(super) dialogue_records: usize,
}

pub(super) struct StableColoringPlan {
    pub(super) glyph_count: usize,
    pub(super) conflict_edge_count: usize,
    pub(super) constructed_clique_glyph_count: usize,
    pub(super) color_count: usize,
    pub(super) assignment_sha1: String,
    pub(super) family_entry_counts: FamilyEntryCounts,
}

pub(super) fn plan_stable_coloring(families: &BattleGlyphFamilies) -> Result<StableColoringPlan> {
    let graph = ConflictGraph::from_families(families);
    let colors = graph.color_deterministically();
    graph.verify_coloring(&colors)?;
    let constructed_clique = constructed_clique(families);
    graph.verify_clique(&constructed_clique)?;
    let color_count = colors
        .iter()
        .copied()
        .max()
        .map_or(0, |maximum| maximum + 1);
    let assignment_sha1 = assignment_sha1(&graph.glyphs, &colors)?;
    Ok(StableColoringPlan {
        glyph_count: graph.glyphs.len(),
        conflict_edge_count: graph.edge_count(),
        constructed_clique_glyph_count: constructed_clique.len(),
        color_count,
        assignment_sha1,
        family_entry_counts: FamilyEntryCounts {
            unit_names: families.unit_names.len(),
            enemy_names: families.enemy_names.len(),
            classes: families.classes.len(),
            items: families.items.len(),
            terrains: families.terrains.len(),
            dialogue_records: families.dialogue_records.len(),
        },
    })
}

fn constructed_clique(families: &BattleGlyphFamilies) -> BTreeSet<char> {
    let always_present = families
        .base
        .iter()
        .copied()
        .chain(union(&families.classes))
        .chain(union(&families.items))
        .chain(union(&families.terrains))
        .collect::<BTreeSet<_>>();
    let mut largest = always_present.clone();
    for unit_name in &families.unit_names {
        for enemy_name in &families.enemy_names {
            for dialogue in &families.dialogue_records {
                let candidate = always_present
                    .iter()
                    .chain(unit_name)
                    .chain(enemy_name)
                    .chain(dialogue)
                    .copied()
                    .collect::<BTreeSet<_>>();
                if candidate.len() > largest.len() {
                    largest = candidate;
                }
            }
        }
    }
    largest
}

struct ConflictGraph {
    glyphs: Vec<char>,
    indices: BTreeMap<char, usize>,
    neighbors: Vec<BTreeSet<usize>>,
}

impl ConflictGraph {
    fn from_families(families: &BattleGlyphFamilies) -> Self {
        let glyphs = all_glyphs(families).into_iter().collect::<Vec<_>>();
        let indices = glyphs
            .iter()
            .copied()
            .enumerate()
            .map(|(index, glyph)| (glyph, index))
            .collect::<BTreeMap<_, _>>();
        let mut graph = Self {
            neighbors: vec![BTreeSet::new(); glyphs.len()],
            glyphs,
            indices,
        };
        graph.add_clique(&families.base);
        for entries in [
            &families.unit_names,
            &families.enemy_names,
            &families.dialogue_records,
        ] {
            for entry in entries {
                graph.add_clique(entry);
            }
        }
        let unit_names = union(&families.unit_names);
        let enemy_names = union(&families.enemy_names);
        let classes = union(&families.classes);
        let items = union(&families.items);
        let terrains = union(&families.terrains);
        let dialogue = union(&families.dialogue_records);
        graph.add_clique(&classes);
        graph.add_clique(&items);
        graph.add_clique(&terrains);
        let fixed_families = [&unit_names, &enemy_names, &classes, &items, &terrains];
        for left in 0..fixed_families.len() {
            for right in left + 1..fixed_families.len() {
                graph.add_cross(fixed_families[left], fixed_families[right]);
            }
        }
        let non_base = fixed_families
            .iter()
            .flat_map(|glyphs| glyphs.iter().copied())
            .chain(dialogue.iter().copied())
            .collect::<BTreeSet<_>>();
        graph.add_cross(&families.base, &non_base);
        for fixed in fixed_families {
            graph.add_cross(&dialogue, fixed);
        }
        graph
    }

    fn add_clique(&mut self, glyphs: &BTreeSet<char>) {
        let glyphs = glyphs.iter().copied().collect::<Vec<_>>();
        for left in 0..glyphs.len() {
            for right in left + 1..glyphs.len() {
                self.add_edge(glyphs[left], glyphs[right]);
            }
        }
    }

    fn add_cross(&mut self, left: &BTreeSet<char>, right: &BTreeSet<char>) {
        for left_glyph in left {
            for right_glyph in right {
                self.add_edge(*left_glyph, *right_glyph);
            }
        }
    }

    fn add_edge(&mut self, left: char, right: char) {
        if left == right {
            return;
        }
        let left = self.indices[&left];
        let right = self.indices[&right];
        self.neighbors[left].insert(right);
        self.neighbors[right].insert(left);
    }

    fn edge_count(&self) -> usize {
        self.neighbors.iter().map(BTreeSet::len).sum::<usize>() / 2
    }

    fn color_deterministically(&self) -> Vec<usize> {
        let mut colors = vec![None; self.glyphs.len()];
        while colors.iter().any(Option::is_none) {
            let mut selected = None;
            let mut selected_saturation = 0;
            let mut selected_degree = 0;
            for vertex in 0..self.glyphs.len() {
                if colors[vertex].is_some() {
                    continue;
                }
                let saturation = self.neighbor_colors(vertex, &colors).len();
                let degree = self.neighbors[vertex].len();
                if selected.is_none()
                    || saturation > selected_saturation
                    || (saturation == selected_saturation && degree > selected_degree)
                {
                    selected = Some(vertex);
                    selected_saturation = saturation;
                    selected_degree = degree;
                }
            }
            let vertex = selected.expect("an uncolored graph has a selectable vertex");
            let used = self.neighbor_colors(vertex, &colors);
            let color = (0..).find(|candidate| !used.contains(candidate)).unwrap();
            colors[vertex] = Some(color);
        }
        colors.into_iter().map(Option::unwrap).collect()
    }

    fn neighbor_colors(&self, vertex: usize, colors: &[Option<usize>]) -> BTreeSet<usize> {
        self.neighbors[vertex]
            .iter()
            .filter_map(|neighbor| colors[*neighbor])
            .collect()
    }

    fn verify_coloring(&self, colors: &[usize]) -> Result<()> {
        ensure!(
            colors.len() == self.glyphs.len(),
            "battle coloring lost a glyph"
        );
        for (vertex, neighbors) in self.neighbors.iter().enumerate() {
            for neighbor in neighbors {
                ensure!(
                    colors[vertex] != colors[*neighbor],
                    "battle coloring assigned one code to conflicting glyphs"
                );
            }
        }
        Ok(())
    }

    fn verify_clique(&self, glyphs: &BTreeSet<char>) -> Result<()> {
        let vertices = glyphs
            .iter()
            .map(|glyph| {
                self.indices
                    .get(glyph)
                    .copied()
                    .context("constructed battle clique contains an unknown glyph")
            })
            .collect::<Result<Vec<_>>>()?;
        for left in 0..vertices.len() {
            for right in left + 1..vertices.len() {
                ensure!(
                    self.neighbors[vertices[left]].contains(&vertices[right]),
                    "constructed battle lower bound is not a clique"
                );
            }
        }
        Ok(())
    }
}

fn all_glyphs(families: &BattleGlyphFamilies) -> BTreeSet<char> {
    families
        .base
        .iter()
        .copied()
        .chain(
            [
                &families.unit_names,
                &families.enemy_names,
                &families.classes,
                &families.items,
                &families.terrains,
                &families.dialogue_records,
            ]
            .into_iter()
            .flat_map(|entries| entries.iter().flat_map(BTreeSet::iter).copied()),
        )
        .collect()
}

fn union(entries: &[BTreeSet<char>]) -> BTreeSet<char> {
    entries.iter().flatten().copied().collect()
}

fn assignment_sha1(glyphs: &[char], colors: &[usize]) -> Result<String> {
    let mut bytes = Vec::new();
    for (glyph, color) in glyphs.iter().zip(colors) {
        bytes.extend_from_slice(glyph.to_string().as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(
            &u16::try_from(*color)
                .context("battle color index exceeds report encoding")?
                .to_le_bytes(),
        );
    }
    Ok(sha1_hex(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(glyphs: &str) -> BTreeSet<char> {
        glyphs.chars().collect()
    }

    #[test]
    fn alternatives_can_share_a_color_but_one_cache_family_cannot() {
        let families = BattleGlyphFamilies {
            base: set("가"),
            unit_names: vec![set("나"), set("다")],
            enemy_names: vec![set("라")],
            classes: vec![set("마"), set("바")],
            items: vec![set("사")],
            terrains: vec![set("아")],
            dialogue_records: vec![set("자"), set("차")],
        };
        let graph = ConflictGraph::from_families(&families);
        let colors = graph.color_deterministically();
        graph.verify_coloring(&colors).unwrap();

        assert_eq!(colors[graph.indices[&'나']], colors[graph.indices[&'다']]);
        assert_eq!(colors[graph.indices[&'자']], colors[graph.indices[&'차']]);
        assert_ne!(colors[graph.indices[&'마']], colors[graph.indices[&'바']]);
        assert_ne!(colors[graph.indices[&'나']], colors[graph.indices[&'라']]);
    }

    #[test]
    fn deterministic_plan_reports_no_glyph_content() {
        let families = BattleGlyphFamilies {
            base: set("가"),
            unit_names: vec![set("나")],
            enemy_names: vec![set("다")],
            classes: vec![set("라")],
            items: vec![set("마")],
            terrains: vec![set("바")],
            dialogue_records: vec![set("사")],
        };
        let first = plan_stable_coloring(&families).unwrap();
        let second = plan_stable_coloring(&families).unwrap();

        assert_eq!(first.assignment_sha1, second.assignment_sha1);
        assert_eq!(first.color_count, second.color_count);
        assert_eq!(first.glyph_count, 7);
        assert!(first.constructed_clique_glyph_count <= first.color_count);
    }
}
