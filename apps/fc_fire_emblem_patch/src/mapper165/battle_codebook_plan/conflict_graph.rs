use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::sha1_hex;

mod ceiling_search;

use ceiling_search::search_coloring;

const ACTIVE_CEILING_SEARCH_NODE_LIMIT: usize = 5_000_000;

pub(super) struct BattleGlyphFamilies {
    pub(super) base: BTreeSet<char>,
    pub(super) participant_modes: Vec<BattleParticipantMode>,
    pub(super) terrains: Vec<BTreeSet<char>>,
    pub(super) dialogue_records: Vec<BTreeSet<char>>,
}

pub(super) struct BattleParticipantMode {
    pub(super) role: &'static str,
    pub(super) player_participants: Vec<BTreeSet<char>>,
    pub(super) enemy_participants: Vec<BTreeSet<char>>,
}

pub(super) struct StableColoringPlan {
    pub(super) glyph_count: usize,
    pub(super) conflict_edge_count: usize,
    pub(super) constructed_clique_glyph_count: usize,
    pub(super) color_count: usize,
    pub(super) assignment_sha1: String,
    pub(super) coloring_strategy: &'static str,
    pub(super) bounded_search_color_ceiling: usize,
    pub(super) bounded_search_node_count: usize,
    pub(super) bounded_search_limit_reached: bool,
    pub(super) bounded_assignment_found: bool,
    pub(super) model_chromatic_number_proven: bool,
    glyph_colors: BTreeMap<char, usize>,
}

impl StableColoringPlan {
    pub(super) fn glyph_colors(&self) -> &BTreeMap<char, usize> {
        &self.glyph_colors
    }

    pub(super) fn expand_to_color_count(&mut self, target_color_count: usize) -> Result<()> {
        ensure!(
            target_color_count >= self.color_count,
            "battle coloring cannot contract from {} to {target_color_count} colors (constructed clique: {}, bounded assignment found: {}, search ceiling: {}, search nodes: {}, limit reached: {})",
            self.color_count,
            self.constructed_clique_glyph_count,
            self.bounded_assignment_found,
            self.bounded_search_color_ceiling,
            self.bounded_search_node_count,
            self.bounded_search_limit_reached,
        );
        ensure!(
            target_color_count <= self.glyph_count,
            "battle coloring cannot assign {target_color_count} colors to {} glyphs",
            self.glyph_count
        );
        let initial_color_count = self.color_count;
        while self.color_count < target_color_count {
            let mut class_sizes = vec![0usize; self.color_count];
            for color in self.glyph_colors.values() {
                class_sizes[*color] += 1;
            }
            let glyph = self
                .glyph_colors
                .iter()
                .rev()
                .find_map(|(glyph, color)| (class_sizes[*color] > 1).then_some(*glyph))
                .context("battle coloring has no color class left to split")?;
            self.glyph_colors.insert(glyph, self.color_count);
            self.color_count += 1;
        }
        if self.color_count != initial_color_count {
            let glyphs = self.glyph_colors.keys().copied().collect::<Vec<_>>();
            let colors = self.glyph_colors.values().copied().collect::<Vec<_>>();
            self.assignment_sha1 = assignment_sha1(&glyphs, &colors)?;
            self.coloring_strategy =
                "bounded coloring followed by deterministic active-width color splitting";
            self.model_chromatic_number_proven = false;
        }
        Ok(())
    }
}

pub(super) fn plan_stable_coloring(
    families: &BattleGlyphFamilies,
    active_color_ceiling: usize,
) -> Result<StableColoringPlan> {
    let graph = ConflictGraph::from_families(families);
    let greedy_colors = graph.color_deterministically();
    graph.verify_coloring(&greedy_colors)?;
    let greedy_color_count = greedy_colors
        .iter()
        .copied()
        .max()
        .map_or(0, |maximum| maximum + 1);
    let constructed_clique = graph.extend_clique(&constructed_clique(families));
    graph.verify_clique(&constructed_clique)?;
    let bounded_search_color_ceiling = active_color_ceiling
        .max(constructed_clique.len())
        .min(greedy_color_count);
    let ceiling_search = search_coloring(
        &graph,
        &constructed_clique,
        bounded_search_color_ceiling,
        ACTIVE_CEILING_SEARCH_NODE_LIMIT,
    );
    let bounded_assignment_found = ceiling_search.colors.is_some();
    let (colors, initial_strategy) = if let Some(colors) = ceiling_search.colors {
        graph.verify_coloring(&colors)?;
        (colors, "clique-seeded bounded ceiling search")
    } else {
        (greedy_colors, "deterministic DSATUR upper bound")
    };
    let color_count = colors
        .iter()
        .copied()
        .max()
        .map_or(0, |maximum| maximum + 1);
    let model_chromatic_number_proven = color_count == constructed_clique.len();
    let coloring_strategy = if model_chromatic_number_proven && color_count > active_color_ceiling {
        "deterministic DSATUR matched the logical-color lower bound above the physical-code ceiling"
    } else if model_chromatic_number_proven {
        "deterministic DSATUR matched constructed clique"
    } else {
        initial_strategy
    };
    let assignment_sha1 = assignment_sha1(&graph.glyphs, &colors)?;
    let glyph_colors = graph
        .glyphs
        .iter()
        .copied()
        .zip(colors.iter().copied())
        .collect();
    Ok(StableColoringPlan {
        glyph_count: graph.glyphs.len(),
        conflict_edge_count: graph.edge_count(),
        constructed_clique_glyph_count: constructed_clique.len(),
        color_count,
        assignment_sha1,
        coloring_strategy,
        bounded_search_color_ceiling,
        bounded_search_node_count: ceiling_search.visited_node_count,
        bounded_search_limit_reached: ceiling_search.node_limit_reached,
        bounded_assignment_found,
        model_chromatic_number_proven,
        glyph_colors,
    })
}

fn constructed_clique(families: &BattleGlyphFamilies) -> BTreeSet<char> {
    let mut clique = families
        .base
        .iter()
        .copied()
        .chain(union(&families.terrains))
        .collect::<BTreeSet<_>>();
    if let Some(entry) = families
        .dialogue_records
        .iter()
        .max_by_key(|entry| entry.difference(&clique).count())
    {
        clique.extend(entry);
    }
    if let Some(participants) = families
        .participant_modes
        .iter()
        .filter_map(|mode| {
            let player = mode
                .player_participants
                .iter()
                .max_by_key(|entry| entry.difference(&clique).count())?;
            let mut participants = player.clone();
            if let Some(enemy) = mode.enemy_participants.iter().max_by_key(|entry| {
                entry
                    .iter()
                    .filter(|glyph| !clique.contains(glyph) && !participants.contains(glyph))
                    .count()
            }) {
                participants.extend(enemy);
            }
            Some(participants)
        })
        .max_by_key(|participants| participants.difference(&clique).count())
    {
        clique.extend(participants);
    }
    clique
}

fn add_participant_choice_cliques(graph: &mut ConflictGraph, mode: &BattleParticipantMode) {
    for entries in [&mode.player_participants, &mode.enemy_participants] {
        for entry in entries {
            graph.add_clique(entry);
        }
    }
}

fn visible_participant_glyphs(families: &BattleGlyphFamilies) -> BTreeSet<char> {
    families
        .participant_modes
        .iter()
        .flat_map(|mode| {
            [&mode.player_participants, &mode.enemy_participants]
                .into_iter()
                .flat_map(|entries| entries.iter().flat_map(BTreeSet::iter).copied())
        })
        .collect()
}

fn participant_mode_glyphs(mode: &BattleParticipantMode) -> (BTreeSet<char>, BTreeSet<char>) {
    (
        union(&mode.player_participants),
        union(&mode.enemy_participants),
    )
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
        for mode in &families.participant_modes {
            add_participant_choice_cliques(&mut graph, mode);
        }
        for entry in &families.dialogue_records {
            graph.add_clique(entry);
        }
        for mode in &families.participant_modes {
            let (player_participants, enemy_participants) = participant_mode_glyphs(mode);
            graph.add_cross(&player_participants, &enemy_participants);
        }
        let terrains = union(&families.terrains);
        let dialogue = union(&families.dialogue_records);
        graph.add_clique(&terrains);
        let visible_participants = visible_participant_glyphs(families);
        graph.add_cross(&terrains, &visible_participants);
        let non_base = visible_participants
            .iter()
            .chain(&terrains)
            .chain(&dialogue)
            .copied()
            .collect::<BTreeSet<_>>();
        graph.add_cross(&families.base, &non_base);
        graph.add_cross(&dialogue, &visible_participants);
        graph.add_cross(&dialogue, &terrains);
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

    fn extend_clique(&self, seed: &BTreeSet<char>) -> BTreeSet<char> {
        let mut clique = seed.clone();
        for glyph in &self.glyphs {
            if clique.contains(glyph) {
                continue;
            }
            let vertex = self.indices[glyph];
            if clique
                .iter()
                .all(|member| self.neighbors[vertex].contains(&self.indices[member]))
            {
                clique.insert(*glyph);
            }
        }
        clique
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

struct ConflictGraph {
    glyphs: Vec<char>,
    indices: BTreeMap<char, usize>,
    neighbors: Vec<BTreeSet<usize>>,
}

fn all_glyphs(families: &BattleGlyphFamilies) -> BTreeSet<char> {
    families
        .base
        .iter()
        .copied()
        .chain(
            families
                .participant_modes
                .iter()
                .flat_map(|mode| {
                    [&mode.player_participants, &mode.enemy_participants]
                        .into_iter()
                        .flat_map(|entries| entries.iter().flat_map(BTreeSet::iter).copied())
                })
                .chain(
                    [&families.terrains, &families.dialogue_records]
                        .into_iter()
                        .flat_map(|entries| entries.iter().flat_map(BTreeSet::iter).copied()),
                ),
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
mod tests;
