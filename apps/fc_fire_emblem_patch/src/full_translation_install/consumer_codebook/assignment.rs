//! 화면 수명 충돌 그래프를 물리 글꼴 코드에 결정적으로 배치한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use super::{CodeOwner, GlyphKey, Lifetime};

pub(super) fn merge_preassignments(
    destination: &mut BTreeMap<GlyphKey, u8>,
    owner: CodeOwner,
    additions: &BTreeMap<char, u8>,
    role: &str,
) -> Result<()> {
    for (glyph, code) in additions {
        let key = GlyphKey {
            owner,
            glyph: *glyph,
        };
        if let Some(previous) = destination.insert(key, *code) {
            ensure!(
                previous == *code,
                "{role} assigns {glyph:?} to {code:02X}, but another source fixed it at {previous:02X}"
            );
        }
    }
    Ok(())
}

pub(super) struct ConflictGraph {
    glyphs: Vec<GlyphKey>,
    neighbors: BTreeMap<GlyphKey, BTreeSet<GlyphKey>>,
}

impl ConflictGraph {
    pub(super) fn from_lifetimes(
        lifetimes: &[Lifetime],
        additional_glyphs: impl Iterator<Item = GlyphKey>,
    ) -> Self {
        let mut glyphs = lifetimes
            .iter()
            .flat_map(|lifetime| lifetime.target_glyphs.iter().copied())
            .chain(additional_glyphs)
            .collect::<BTreeSet<_>>();
        let mut neighbors = glyphs
            .iter()
            .copied()
            .map(|glyph| (glyph, BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        for lifetime in lifetimes {
            let visible = lifetime.target_glyphs.iter().copied().collect::<Vec<_>>();
            for left in 0..visible.len() {
                for right in left + 1..visible.len() {
                    neighbors
                        .get_mut(&visible[left])
                        .unwrap()
                        .insert(visible[right]);
                    neighbors
                        .get_mut(&visible[right])
                        .unwrap()
                        .insert(visible[left]);
                }
            }
        }
        Self {
            glyphs: std::mem::take(&mut glyphs).into_iter().collect(),
            neighbors,
        }
    }

    pub(super) fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    pub(super) fn edge_count(&self) -> usize {
        self.neighbors.values().map(BTreeSet::len).sum::<usize>() / 2
    }
}

pub(super) fn assign_codes(
    graph: &ConflictGraph,
    forbidden: &BTreeMap<GlyphKey, BTreeSet<u8>>,
    preassigned: &BTreeMap<GlyphKey, u8>,
    active_codes: &BTreeSet<u8>,
) -> Result<(BTreeMap<GlyphKey, u8>, &'static str, usize)> {
    verify_preassignments(graph, forbidden, preassigned, active_codes)?;
    let mut colors = abstract_dsatur(graph, preassigned);
    let mut color_split_count = 0;
    let color_codes = loop {
        let color_count = colors.values().copied().max().map_or(0, |color| color + 1);
        ensure!(
            color_count <= active_codes.len(),
            "consumer fixed-UI graph needs {color_count} distinct colors but only {} physical codes are active",
            active_codes.len()
        );
        if let Some(color_codes) = match_color_codes(
            graph,
            forbidden,
            preassigned,
            &colors,
            active_codes,
            color_count,
        ) {
            break color_codes;
        }
        split_one_color(&mut colors, forbidden)?;
        color_split_count += 1;
    };
    let mut assignments = preassigned.clone();
    assignments.extend(
        colors
            .iter()
            .map(|(glyph, color)| (*glyph, color_codes[*color])),
    );
    Ok((
        assignments,
        "precolored DSATUR followed by deterministic color-to-code matching",
        color_split_count,
    ))
}

fn verify_preassignments(
    graph: &ConflictGraph,
    forbidden: &BTreeMap<GlyphKey, BTreeSet<u8>>,
    preassigned: &BTreeMap<GlyphKey, u8>,
    active_codes: &BTreeSet<u8>,
) -> Result<()> {
    for (glyph, code) in preassigned {
        ensure!(
            active_codes.contains(code)
                && !forbidden
                    .get(glyph)
                    .is_some_and(|codes| codes.contains(code)),
            "consumer preassignment {glyph:?}={code:02X} is not valid in every lifetime"
        );
        for neighbor in &graph.neighbors[glyph] {
            ensure!(
                preassigned.get(neighbor) != Some(code),
                "consumer lifetime fixes code {code:02X} for both {glyph:?} and {neighbor:?}"
            );
        }
    }
    Ok(())
}

fn abstract_dsatur(
    graph: &ConflictGraph,
    preassigned: &BTreeMap<GlyphKey, u8>,
) -> BTreeMap<GlyphKey, usize> {
    let mut colors = BTreeMap::<GlyphKey, usize>::new();
    while colors.len() + preassigned.len() < graph.glyphs.len() {
        let glyph = graph
            .glyphs
            .iter()
            .copied()
            .filter(|glyph| !preassigned.contains_key(glyph) && !colors.contains_key(glyph))
            .max_by_key(|glyph| {
                let saturation = graph.neighbors[glyph]
                    .iter()
                    .filter_map(|neighbor| colors.get(neighbor))
                    .copied()
                    .collect::<BTreeSet<_>>()
                    .len();
                (
                    saturation,
                    graph.neighbors[glyph].len(),
                    std::cmp::Reverse(*glyph),
                )
            })
            .expect("an uncolored consumer glyph is selectable");
        let used = graph.neighbors[&glyph]
            .iter()
            .filter_map(|neighbor| colors.get(neighbor))
            .copied()
            .collect::<BTreeSet<_>>();
        let color = (0..).find(|color| !used.contains(color)).unwrap();
        colors.insert(glyph, color);
    }
    colors
}

fn match_color_codes(
    graph: &ConflictGraph,
    forbidden: &BTreeMap<GlyphKey, BTreeSet<u8>>,
    preassigned: &BTreeMap<GlyphKey, u8>,
    colors: &BTreeMap<GlyphKey, usize>,
    active_codes: &BTreeSet<u8>,
    color_count: usize,
) -> Option<Vec<u8>> {
    let mut forbidden_by_color = vec![BTreeSet::<u8>::new(); color_count];
    for (glyph, color) in colors {
        forbidden_by_color[*color].extend(forbidden.get(glyph).into_iter().flatten().copied());
        forbidden_by_color[*color].extend(
            graph.neighbors[glyph]
                .iter()
                .filter_map(|neighbor| preassigned.get(neighbor))
                .copied(),
        );
    }
    let mut order = (0..color_count).collect::<Vec<_>>();
    order.sort_by_key(|color| (std::cmp::Reverse(forbidden_by_color[*color].len()), *color));
    let active_codes = active_codes.iter().copied().collect::<Vec<_>>();
    let mut owner_by_code = BTreeMap::<u8, usize>::new();
    let mut code_by_color = vec![None; color_count];
    for color in order {
        let mut visited = BTreeSet::new();
        if !match_one_color(
            color,
            &active_codes,
            &forbidden_by_color,
            &mut visited,
            &mut owner_by_code,
            &mut code_by_color,
        ) {
            return None;
        }
    }
    code_by_color.into_iter().collect()
}

fn match_one_color(
    color: usize,
    active_codes: &[u8],
    forbidden_by_color: &[BTreeSet<u8>],
    visited: &mut BTreeSet<u8>,
    owner_by_code: &mut BTreeMap<u8, usize>,
    code_by_color: &mut [Option<u8>],
) -> bool {
    for code in active_codes {
        if forbidden_by_color[color].contains(code) || !visited.insert(*code) {
            continue;
        }
        let previous = owner_by_code.get(code).copied();
        if previous.is_none_or(|owner| {
            match_one_color(
                owner,
                active_codes,
                forbidden_by_color,
                visited,
                owner_by_code,
                code_by_color,
            )
        }) {
            owner_by_code.insert(*code, color);
            code_by_color[color] = Some(*code);
            return true;
        }
    }
    false
}

fn split_one_color(
    colors: &mut BTreeMap<GlyphKey, usize>,
    forbidden: &BTreeMap<GlyphKey, BTreeSet<u8>>,
) -> Result<()> {
    let mut members = BTreeMap::<usize, Vec<GlyphKey>>::new();
    for (glyph, color) in colors.iter() {
        members.entry(*color).or_default().push(*glyph);
    }
    let color = members
        .iter()
        .filter(|(_, glyphs)| glyphs.len() > 1)
        .max_by_key(|(color, glyphs)| {
            let forbidden_count = glyphs
                .iter()
                .flat_map(|glyph| forbidden.get(glyph).into_iter().flatten().copied())
                .collect::<BTreeSet<_>>()
                .len();
            (forbidden_count, glyphs.len(), std::cmp::Reverse(**color))
        })
        .map(|(color, _)| *color)
        .context("consumer color matching failed after every color became a singleton")?;
    let glyph = members[&color]
        .iter()
        .copied()
        .max_by_key(|glyph| (forbidden.get(glyph).map_or(0, BTreeSet::len), *glyph))
        .expect("a splittable color has a glyph");
    let next_color = colors.values().copied().max().map_or(0, |color| color + 1);
    colors.insert(glyph, next_color);
    Ok(())
}

pub(super) fn verify_assignment(
    graph: &ConflictGraph,
    forbidden: &BTreeMap<GlyphKey, BTreeSet<u8>>,
    preassigned: &BTreeMap<GlyphKey, u8>,
    assignments: &BTreeMap<GlyphKey, u8>,
) -> Result<()> {
    ensure!(
        assignments.len() == graph.glyphs.len()
            && preassigned
                .iter()
                .all(|(glyph, code)| assignments.get(glyph) == Some(code)),
        "consumer assignment lost a glyph or changed a preassignment"
    );
    for glyph in &graph.glyphs {
        let code = assignments[glyph];
        ensure!(
            !forbidden
                .get(glyph)
                .is_some_and(|codes| codes.contains(&code)),
            "consumer glyph {glyph:?} overwrites preserved code {code:02X}"
        );
        for neighbor in &graph.neighbors[glyph] {
            ensure!(
                assignments[neighbor] != code,
                "consumer lifetime assigns code {code:02X} to both {glyph:?} and {neighbor:?}"
            );
        }
    }
    Ok(())
}

pub(super) fn assignment_sha1(assignments: &BTreeMap<GlyphKey, u8>) -> String {
    let mut bytes = Vec::new();
    for (key, code) in assignments {
        bytes.push(match key.owner {
            CodeOwner::DialogueDynamic => 0,
            CodeOwner::ChapterTitle => 1,
            CodeOwner::FixedUi => 2,
            CodeOwner::OptionsTable => 3,
        });
        bytes.extend_from_slice(key.glyph.to_string().as_bytes());
        bytes.push(*code);
    }
    crate::sha1_hex(&bytes)
}

#[cfg(test)]
mod tests {
    use crate::font_slots::active_hangul_codes;

    use super::*;
    use crate::full_translation_install::consumer_codebook::lifetimes::forbidden_codes_by_glyph;

    fn synthetic_lifetime(id: &'static str, glyphs: &str) -> Lifetime {
        Lifetime {
            id,
            variant: "synthetic",
            screen_roles: vec![id],
            domain_ids: vec![id],
            target_glyphs: glyphs
                .chars()
                .map(|glyph| GlyphKey {
                    owner: CodeOwner::FixedUi,
                    glyph,
                })
                .collect(),
            preserved_active_codes: BTreeSet::new(),
            emit_static_page: true,
        }
    }

    #[test]
    fn non_coexistent_glyphs_may_reuse_one_physical_code() {
        let lifetimes = [
            synthetic_lifetime("left", "가나"),
            synthetic_lifetime("right", "다라"),
        ];
        let graph = ConflictGraph::from_lifetimes(&lifetimes, std::iter::empty());
        let active = active_hangul_codes().into_iter().collect();
        let (assignment, _, _) =
            assign_codes(&graph, &BTreeMap::new(), &BTreeMap::new(), &active).unwrap();

        let key = |glyph| GlyphKey {
            owner: CodeOwner::FixedUi,
            glyph,
        };
        assert_ne!(assignment[&key('가')], assignment[&key('나')]);
        assert_ne!(assignment[&key('다')], assignment[&key('라')]);
        assert_eq!(
            assignment.values().copied().collect::<BTreeSet<_>>().len(),
            2
        );
    }

    #[test]
    fn precolored_and_preserved_codes_are_hard_constraints() {
        let mut lifetime = synthetic_lifetime("screen", "가나다");
        let active = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
        let first = *active.first().unwrap();
        let second = *active.iter().nth(1).unwrap();
        lifetime.preserved_active_codes.insert(second);
        let graph = ConflictGraph::from_lifetimes(&[lifetime.clone()], std::iter::empty());
        let forbidden = forbidden_codes_by_glyph(&[lifetime]);
        let key = GlyphKey {
            owner: CodeOwner::FixedUi,
            glyph: '가',
        };
        let preassigned = BTreeMap::from([(key, first)]);

        let (assignment, _, _) = assign_codes(&graph, &forbidden, &preassigned, &active).unwrap();

        assert_eq!(assignment[&key], first);
        assert!(assignment.values().all(|code| *code != second));
        verify_assignment(&graph, &forbidden, &preassigned, &assignment).unwrap();
    }
}
