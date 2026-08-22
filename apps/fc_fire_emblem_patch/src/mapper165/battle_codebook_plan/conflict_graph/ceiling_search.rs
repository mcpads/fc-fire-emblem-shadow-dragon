use std::collections::BTreeSet;

use super::ConflictGraph;

pub(super) struct CeilingColoringSearch {
    pub(super) colors: Option<Vec<usize>>,
    pub(super) visited_node_count: usize,
    pub(super) node_limit_reached: bool,
}

pub(super) fn search_coloring(
    graph: &ConflictGraph,
    clique_glyphs: &BTreeSet<char>,
    color_ceiling: usize,
    node_limit: usize,
) -> CeilingColoringSearch {
    if clique_glyphs.len() > color_ceiling {
        return CeilingColoringSearch {
            colors: None,
            visited_node_count: 0,
            node_limit_reached: false,
        };
    }
    let mut colors = vec![None; graph.glyphs.len()];
    for (color, glyph) in clique_glyphs.iter().enumerate() {
        colors[graph.indices[glyph]] = Some(color);
    }
    let mut state = SearchState {
        graph,
        color_ceiling,
        node_limit,
        visited_node_count: 0,
        node_limit_reached: false,
    };
    let found = state.assign_remaining(&mut colors, clique_glyphs.len());
    CeilingColoringSearch {
        colors: found.then(|| colors.into_iter().map(Option::unwrap).collect()),
        visited_node_count: state.visited_node_count,
        node_limit_reached: state.node_limit_reached,
    }
}

struct SearchState<'a> {
    graph: &'a ConflictGraph,
    color_ceiling: usize,
    node_limit: usize,
    visited_node_count: usize,
    node_limit_reached: bool,
}

impl SearchState<'_> {
    fn assign_remaining(&mut self, colors: &mut [Option<usize>], used_color_count: usize) -> bool {
        if colors.iter().all(Option::is_some) {
            return true;
        }
        if self.visited_node_count >= self.node_limit {
            self.node_limit_reached = true;
            return false;
        }
        self.visited_node_count += 1;

        let Some((vertex, mut candidates)) = self.select_vertex(colors, used_color_count) else {
            return false;
        };
        candidates.sort_by_key(|color| {
            (
                self.uncolored_neighbor_impact(vertex, *color, colors),
                usize::from(*color == used_color_count),
                *color,
            )
        });
        for color in candidates {
            colors[vertex] = Some(color);
            let next_used_color_count = used_color_count.max(color + 1);
            if self.assign_remaining(colors, next_used_color_count) {
                return true;
            }
            colors[vertex] = None;
            if self.node_limit_reached {
                return false;
            }
        }
        false
    }

    fn select_vertex(
        &self,
        colors: &[Option<usize>],
        used_color_count: usize,
    ) -> Option<(usize, Vec<usize>)> {
        let mut selected: Option<(usize, Vec<usize>, usize, usize)> = None;
        for vertex in 0..self.graph.glyphs.len() {
            if colors[vertex].is_some() {
                continue;
            }
            let candidates = self.available_colors(vertex, colors, used_color_count);
            if candidates.is_empty() {
                return None;
            }
            let saturation = self.neighbor_color_count(vertex, colors);
            let degree = self.graph.neighbors[vertex].len();
            let should_select = selected.as_ref().is_none_or(
                |(selected_vertex, selected_candidates, selected_saturation, selected_degree)| {
                    candidates.len() < selected_candidates.len()
                        || (candidates.len() == selected_candidates.len()
                            && saturation > *selected_saturation)
                        || (candidates.len() == selected_candidates.len()
                            && saturation == *selected_saturation
                            && degree > *selected_degree)
                        || (candidates.len() == selected_candidates.len()
                            && saturation == *selected_saturation
                            && degree == *selected_degree
                            && vertex < *selected_vertex)
                },
            );
            if should_select {
                selected = Some((vertex, candidates, saturation, degree));
            }
        }
        selected.map(|(vertex, candidates, _, _)| (vertex, candidates))
    }

    fn available_colors(
        &self,
        vertex: usize,
        colors: &[Option<usize>],
        used_color_count: usize,
    ) -> Vec<usize> {
        let mut blocked = vec![false; self.color_ceiling];
        for neighbor in &self.graph.neighbors[vertex] {
            if let Some(color) = colors[*neighbor] {
                blocked[color] = true;
            }
        }
        let mut available = (0..used_color_count)
            .filter(|color| !blocked[*color])
            .collect::<Vec<_>>();
        if used_color_count < self.color_ceiling {
            available.push(used_color_count);
        }
        available
    }

    fn neighbor_color_count(&self, vertex: usize, colors: &[Option<usize>]) -> usize {
        self.graph.neighbors[vertex]
            .iter()
            .filter_map(|neighbor| colors[*neighbor])
            .collect::<BTreeSet<_>>()
            .len()
    }

    fn uncolored_neighbor_impact(
        &self,
        vertex: usize,
        color: usize,
        colors: &[Option<usize>],
    ) -> usize {
        self.graph.neighbors[vertex]
            .iter()
            .filter(|neighbor| {
                colors[**neighbor].is_none()
                    && self.graph.neighbors[**neighbor]
                        .iter()
                        .all(|other| colors[*other] != Some(color))
            })
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn graph(glyphs: &[char], edges: &[(usize, usize)]) -> ConflictGraph {
        let mut neighbors = vec![BTreeSet::new(); glyphs.len()];
        for (left, right) in edges {
            neighbors[*left].insert(*right);
            neighbors[*right].insert(*left);
        }
        ConflictGraph {
            glyphs: glyphs.to_vec(),
            indices: glyphs
                .iter()
                .copied()
                .enumerate()
                .map(|(index, glyph)| (glyph, index))
                .collect::<BTreeMap<_, _>>(),
            neighbors,
        }
    }

    #[test]
    fn clique_seed_finds_a_ceiling_assignment_without_renaming_colors() {
        let graph = graph(&['가', '나', '다'], &[(0, 1), (1, 2)]);
        let clique = ['가', '나'].into_iter().collect();

        let search = search_coloring(&graph, &clique, 2, 100);

        let colors = search.colors.unwrap();
        graph.verify_coloring(&colors).unwrap();
        assert_eq!(colors[0], colors[2]);
        assert!(!search.node_limit_reached);
    }

    #[test]
    fn clique_larger_than_the_ceiling_is_an_immediate_proof() {
        let graph = graph(&['가', '나'], &[(0, 1)]);
        let clique = ['가', '나'].into_iter().collect();

        let search = search_coloring(&graph, &clique, 1, 100);

        assert!(search.colors.is_none());
        assert_eq!(search.visited_node_count, 0);
        assert!(!search.node_limit_reached);
    }
}
