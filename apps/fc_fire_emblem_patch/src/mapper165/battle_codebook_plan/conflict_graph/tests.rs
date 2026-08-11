use super::*;

fn set(glyphs: &str) -> BTreeSet<char> {
    glyphs.chars().collect()
}

#[test]
fn alternatives_can_share_a_color_but_one_cache_family_cannot() {
    let families = BattleGlyphFamilies {
        base: set("가"),
        player_participants: vec![set("나마사"), set("다바사")],
        enemy_participants: vec![set("라")],
        terrains: vec![set("아")],
        dialogue_records: vec![set("자"), set("차")],
    };
    let graph = ConflictGraph::from_families(&families);
    let colors = graph.color_deterministically();
    graph.verify_coloring(&colors).unwrap();

    assert_eq!(colors[graph.indices[&'나']], colors[graph.indices[&'다']]);
    assert_eq!(colors[graph.indices[&'자']], colors[graph.indices[&'차']]);
    assert_eq!(colors[graph.indices[&'마']], colors[graph.indices[&'바']]);
    assert_ne!(colors[graph.indices[&'나']], colors[graph.indices[&'마']]);
    assert_ne!(colors[graph.indices[&'마']], colors[graph.indices[&'사']]);
    assert_ne!(colors[graph.indices[&'나']], colors[graph.indices[&'라']]);
}

#[test]
fn deterministic_plan_reports_no_glyph_content() {
    let families = BattleGlyphFamilies {
        base: set("가"),
        player_participants: vec![set("나라마")],
        enemy_participants: vec![set("다")],
        terrains: vec![set("바")],
        dialogue_records: vec![set("사")],
    };
    let first = plan_stable_coloring(&families, 7).unwrap();
    let second = plan_stable_coloring(&families, 7).unwrap();

    assert_eq!(first.assignment_sha1, second.assignment_sha1);
    assert_eq!(first.color_count, second.color_count);
    assert_eq!(first.glyph_count, 7);
    assert!(first.constructed_clique_glyph_count <= first.color_count);
    assert!(first.active_ceiling_assignment_found);
    assert!(!first.active_ceiling_search_limit_reached);
    assert!(first.model_chromatic_number_proven);
}

#[test]
fn clique_extension_adds_only_vertices_adjacent_to_every_member() {
    let families = BattleGlyphFamilies {
        base: set("가"),
        player_participants: vec![set("나다마바")],
        enemy_participants: vec![set("라")],
        terrains: vec![set("사")],
        dialogue_records: vec![set("아")],
    };
    let graph = ConflictGraph::from_families(&families);
    let seed = ['가', '나', '라', '마', '바', '사', '아']
        .into_iter()
        .collect();

    let extended = graph.extend_clique(&seed);

    assert!(extended.contains(&'다'));
    graph.verify_clique(&extended).unwrap();
}

#[test]
fn color_classes_can_expand_to_a_fixed_runtime_codebook_width() {
    let families = BattleGlyphFamilies {
        base: set("가"),
        player_participants: vec![set("나"), set("다")],
        enemy_participants: vec![],
        terrains: vec![],
        dialogue_records: vec![],
    };
    let mut first = plan_stable_coloring(&families, 3).unwrap();
    let mut second = plan_stable_coloring(&families, 3).unwrap();
    assert_eq!(first.color_count, 2);

    first.expand_to_color_count(3).unwrap();
    second.expand_to_color_count(3).unwrap();

    assert_eq!(first.color_count, 3);
    assert_eq!(first.assignment_sha1, second.assignment_sha1);
    assert_eq!(
        first
            .glyph_colors()
            .values()
            .copied()
            .collect::<BTreeSet<_>>()
            .len(),
        3
    );
    assert!(!first.model_chromatic_number_proven);
}
