use super::*;

fn population(name: &str, units: &[(&str, &str)]) -> GlyphPopulation {
    GlyphPopulation {
        name: name.to_owned(),
        kind: "test",
        units: units
            .iter()
            .map(|(id, text)| TextUnit {
                id: (*id).to_owned(),
                text: (*text).to_owned(),
            })
            .collect(),
    }
}

fn demand(summary: &PopulationSummary, glyph: char) -> &GlyphDemand {
    summary
        .glyphs
        .iter()
        .find(|demand| demand.glyph == glyph)
        .unwrap_or_else(|| panic!("summary has no demand for {glyph:?}"))
}

const MAIN_DIALOGUE_WORKSPACE: &str = r#"{
  "source_sha1": "0179c550d424e0397496078789e7b116601d120c",
  "records": [
    {
      "id": "shop-and-item-dialogue:006",
      "table_id": "shop-and-item-dialogue",
      "lines": [{ "korean": "{EA}또와줘{EF}" }]
    },
    {
      "id": "shop-and-item-dialogue:045",
      "table_id": "shop-and-item-dialogue",
      "lines": [{ "korean": "{EA}또보자{E7}" }]
    },
    {
      "id": "house-dialogue:000",
      "table_id": "house-dialogue",
      "lines": [{ "korean": "{EA}어서와{ED}" }, { "korean": "쉬어가{EF}" }]
    }
  ]
}"#;

const FIXED_TEXT_WORKSPACE: &str = r#"{
  "source_sha1": "0179c550d424e0397496078789e7b116601d120c",
  "entries": [
    { "id": "item-names:000", "table_id": "item-names", "korean_markup": "은검" },
    { "id": "item-names:001", "table_id": "item-names", "korean_markup": "철검" },
    { "id": "class-names:000", "table_id": "class-names", "korean_markup": "사회사" }
  ]
}"#;

#[test]
fn builds_one_population_per_main_dialogue_table() {
    let populations = main_dialogue_populations(MAIN_DIALOGUE_WORKSPACE.as_bytes()).unwrap();

    assert_eq!(populations.len(), 2);
    assert_eq!(populations[0].name, "house-dialogue");
    assert_eq!(populations[0].kind, "main_dialogue_table");
    assert_eq!(populations[0].units.len(), 1);
    assert_eq!(populations[1].name, "shop-and-item-dialogue");
    assert_eq!(populations[1].units.len(), 2);

    let house = summarize_population(&populations[0]);
    assert_eq!(house.glyph_count, 5);
    assert_eq!(demand(&house, '어').occurrence_count, 2);
}

#[test]
fn builds_one_population_per_fixed_text_table() {
    let populations = fixed_text_populations(FIXED_TEXT_WORKSPACE.as_bytes()).unwrap();

    assert_eq!(populations.len(), 2);
    assert_eq!(populations[0].name, "class-names");
    assert_eq!(populations[0].kind, "fixed_text_table");
    assert_eq!(populations[1].name, "item-names");
    assert_eq!(populations[1].units.len(), 2);
    assert_eq!(summarize_population(&populations[1]).glyph_count, 3);
}

#[test]
fn a_named_list_spec_splits_its_name_from_its_members() {
    let spec =
        parse_named_list("storage-lifetime=shop-and-item-dialogue:006,house-dialogue:000").unwrap();

    assert_eq!(spec.name, "storage-lifetime");
    assert_eq!(
        spec.members,
        ["shop-and-item-dialogue:006", "house-dialogue:000"]
    );
}

#[test]
fn a_named_list_spec_rejects_a_missing_name_separator() {
    let error = parse_named_list("shop-and-item-dialogue:006").unwrap_err();

    assert!(error.to_string().contains("NAME=MEMBER"));
}

#[test]
fn the_report_refuses_workspaces_bound_to_different_sources() {
    let mismatched = FIXED_TEXT_WORKSPACE.replace(
        "0179c550d424e0397496078789e7b116601d120c",
        "0000000000000000000000000000000000000000",
    );

    let error = build_report(
        MAIN_DIALOGUE_WORKSPACE.as_bytes(),
        mismatched.as_bytes(),
        &[],
        &[],
        210,
    )
    .unwrap_err();

    assert!(error.to_string().contains("same source"));
}

#[test]
fn an_ad_hoc_population_selects_units_by_id_across_tables() {
    let tables = main_dialogue_populations(MAIN_DIALOGUE_WORKSPACE.as_bytes()).unwrap();

    let selected = select_population(
        "storage-lifetime",
        &tables,
        &[
            "shop-and-item-dialogue:006".to_owned(),
            "house-dialogue:000".to_owned(),
        ],
    )
    .unwrap();

    assert_eq!(selected.name, "storage-lifetime");
    assert_eq!(selected.kind, "selected_units");
    assert_eq!(selected.units.len(), 2);
    // 또 와 줘 + 어 서 와 쉬 어 가 에서 `와`와 `어`가 겹쳐 일곱 자다.
    assert_eq!(summarize_population(&selected).glyph_count, 7);
}

#[test]
fn an_ad_hoc_population_rejects_a_unit_id_the_workspaces_do_not_hold() {
    let tables = main_dialogue_populations(MAIN_DIALOGUE_WORKSPACE.as_bytes()).unwrap();

    let error = select_population(
        "storage-lifetime",
        &tables,
        &["shop-and-item-dialogue:999".to_owned()],
    )
    .unwrap_err();

    assert!(error.to_string().contains("shop-and-item-dialogue:999"));
}

#[test]
fn a_coresident_set_unions_its_populations_against_the_slot_budget() {
    let dialogue = summarize_population(&population("dialogue", &[("record:006", "또와줘")]));
    let labels = summarize_population(&population("labels", &[("label:35", "보관줘")]));

    let set = evaluate_coresident_set("storage-screen", &[&dialogue, &labels], 5);

    assert_eq!(set.population_names, ["dialogue", "labels"]);
    assert_eq!(set.glyph_count, 5);
    assert_eq!(set.slot_budget, 5);
    assert!(set.fits);
    assert_eq!(set.excess_glyph_count, 0);
}

#[test]
fn a_coresident_set_ranks_the_glyphs_that_are_cheapest_to_remove_first() {
    let dialogue = summarize_population(&population(
        "dialogue",
        &[("record:006", "가가가나"), ("record:041", "가나")],
    ));
    let labels = summarize_population(&population("labels", &[("label:35", "다")]));

    let set = evaluate_coresident_set("storage-screen", &[&dialogue, &labels], 3);

    let candidates = &set.reduction_candidates;
    assert_eq!(candidates.len(), 3);
    assert_eq!(candidates[0].glyph, '다');
    assert_eq!(candidates[0].occurrence_count, 1);
    assert_eq!(candidates[0].unit_ids, ["label:35"]);
    assert_eq!(candidates[1].glyph, '나');
    assert_eq!(candidates[1].occurrence_count, 2);
    assert_eq!(candidates[1].unit_ids, ["record:006", "record:041"]);
    assert_eq!(candidates[2].glyph, '가');
    assert_eq!(candidates[2].occurrence_count, 4);
}

#[test]
fn a_coresident_set_reports_how_many_glyphs_exceed_the_slot_budget() {
    let dialogue = summarize_population(&population("dialogue", &[("record:006", "또와줘")]));
    let labels = summarize_population(&population("labels", &[("label:35", "보관줘")]));

    let set = evaluate_coresident_set("storage-screen", &[&dialogue, &labels], 3);

    assert!(!set.fits);
    assert_eq!(set.excess_glyph_count, 2);
}

#[test]
fn ignores_control_markup_and_codes_the_source_keeps() {
    let summary = summarize_population(&population(
        "storage",
        &[
            ("record:042", "{E9:02}{EA}하나당10골드{ED}"),
            ("record:067", "{EC:00}{SP}을(를){EF}"),
        ],
    ));

    assert_eq!(summary.glyph_count, 7);
    assert_eq!(summary.occurrence_count, 7);
    assert!(summary.glyphs.iter().all(|demand| demand.glyph > '\u{7f}'));
}

#[test]
fn counts_each_glyph_by_occurrence_and_by_unit_spread() {
    let summary = summarize_population(&population(
        "storage",
        &[("record:006", "또와줘"), ("record:041", "또보자")],
    ));

    assert_eq!(summary.unit_count, 2);
    assert_eq!(summary.occurrence_count, 6);
    assert_eq!(summary.glyph_count, 5);
    assert_eq!(demand(&summary, '또').occurrence_count, 2);
    assert_eq!(demand(&summary, '또').unit_count, 2);
    assert_eq!(demand(&summary, '와').occurrence_count, 1);
    assert_eq!(demand(&summary, '와').unit_count, 1);
}
