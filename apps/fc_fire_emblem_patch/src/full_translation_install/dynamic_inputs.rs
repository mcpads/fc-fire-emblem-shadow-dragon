use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_assets::{MainDialogueDisplayPlan, ending_character_epilogue_preserved_active_codes},
    font_slots::active_hangul_codes,
    mapper165::battle_codebook_plan::GlyphWorkset,
    text_inventory::FixedTextPlannedEntry,
};

use super::transition_residency::TransitionLifetimeWorksets;

mod page_code_identity;
mod producer_encoding;

pub(in crate::full_translation_install) use page_code_identity::{
    DynamicStringPageCodePlan, bind_dynamic_string_page_codes,
};
pub(in crate::full_translation_install) use producer_encoding::{
    DynamicProducerEncodingPlan, bind_dynamic_producer_encoding,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum DynamicStringDomain {
    PreservedNumeric,
    ItemName,
    PlayableUnitName,
    LocationName,
}

pub(super) struct DynamicDialogueInputPlan {
    pub(super) augmented_worksets: Vec<GlyphWorkset>,
    dynamic_glyphs_by_workset: Vec<BTreeSet<char>>,
    translated_dynamic_by_workset: Vec<bool>,
    preserved_numeric_by_workset: Vec<bool>,
    canonical_dynamic_codes: BTreeMap<char, u8>,
    pub(super) declared_domain_count: usize,
    pub(super) translated_dynamic_page_count: usize,
    pub(super) preserved_numeric_page_count: usize,
    pub(super) translated_dynamic_glyph_count: usize,
    pub(super) combined_dialogue_glyph_count: usize,
    pub(super) maximum_possible_domain_glyph_count: usize,
    pub(super) maximum_augmented_workset_slot_demand: usize,
    pub(super) maximum_rendered_target_glyph_upper_bound: usize,
    pub(super) mixed_dynamic_domain_page_count: usize,
    pub(super) every_dynamic_control_classified: bool,
    pub(super) every_augmented_workset_fits: bool,
}

impl DynamicDialogueInputPlan {
    /// 대사 안에서 동적으로 들어오는 아이템·유닛·지명은 저장 바이트와 글꼴 코드가
    /// 이미 한 계약이다. 고정 UI 코드북도 이 값을 씨앗으로 써야 같은 원천 표를
    /// 소비자마다 다시 인코딩하지 않는다.
    pub(super) fn canonical_dynamic_codes(&self) -> &BTreeMap<char, u8> {
        &self.canonical_dynamic_codes
    }
}

pub(super) fn plan_dynamic_dialogue_inputs(
    dialogue: &MainDialogueDisplayPlan,
    fixed_text: &[FixedTextPlannedEntry],
    unit_names: &[FixedTextPlannedEntry],
    location_names: &[FixedTextPlannedEntry],
    transition_lifetimes: &[TransitionLifetimeWorksets],
) -> Result<DynamicDialogueInputPlan> {
    let item_name_domain = domain_glyphs(fixed_text, "item-names")?;
    let unit_name_domain = domain_glyphs(unit_names, "unit-names")?;
    let location_name_domain = domain_glyphs(location_names, "location-names")?;
    let domains = BTreeMap::from([
        (DynamicStringDomain::ItemName, item_name_domain),
        (DynamicStringDomain::PlayableUnitName, unit_name_domain),
        (DynamicStringDomain::LocationName, location_name_domain),
    ]);
    let translated_dynamic_glyphs = domains
        .values()
        .flat_map(|domain| domain.glyphs.iter().copied())
        .collect::<BTreeSet<_>>();
    let literal_dialogue_glyphs = dialogue.unique_glyphs();
    let combined_dialogue_glyph_count = literal_dialogue_glyphs
        .union(&translated_dynamic_glyphs)
        .count();
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    ensure!(
        translated_dynamic_glyphs.len() <= active_codes.len(),
        "dynamic dialogue canonical domain exceeds one physical codebook"
    );

    let mut augmented_worksets = Vec::with_capacity(dialogue.page_worksets.len());
    let mut dynamic_glyphs_by_workset = Vec::with_capacity(dialogue.page_worksets.len());
    let mut translated_dynamic_by_workset = Vec::with_capacity(dialogue.page_worksets.len());
    let mut preserved_numeric_by_workset = Vec::with_capacity(dialogue.page_worksets.len());
    let mut translated_dynamic_page_count = 0;
    let mut preserved_numeric_page_count = 0;
    let mut maximum_possible_domain_glyph_count = 0;
    let mut maximum_augmented_workset_slot_demand = 0;
    let mut maximum_rendered_target_glyph_upper_bound = 0;
    let mut mixed_dynamic_domain_page_count = 0;
    let mut classified_control_count = 0;

    for workset in &dialogue.page_worksets {
        let mut target_glyphs = workset.target_glyphs.clone();
        let mut preserved_active_codes = workset.preserved_target_active_codes.clone();
        if is_ending_character_epilogue_record(&workset.record_id) {
            preserved_active_codes.extend(ending_character_epilogue_preserved_active_codes());
        }
        let mut possible_domain_glyphs = BTreeSet::new();
        let mut rendered_dynamic_glyph_upper_bound = 0;
        let mut has_translated_domain = false;
        let mut has_preserved_numeric = false;

        for (selector, control_count) in &workset.dynamic_string_selector_counts {
            let domain = dynamic_string_domain(&workset.record_id, *selector).ok_or_else(|| {
                anyhow::anyhow!(
                    "{} page {} has an unclassified EC selector {selector:02X}",
                    workset.record_id,
                    workset.page_index
                )
            })?;
            classified_control_count += *control_count;
            match domain {
                DynamicStringDomain::PreservedNumeric => has_preserved_numeric = true,
                translated => {
                    has_translated_domain = true;
                    let glyphs = possible_dynamic_glyphs(
                        &workset.record_id,
                        *selector,
                        translated,
                        &domains,
                        unit_names,
                    )?;
                    let maximum_entry_glyph_count = if translated
                        == DynamicStringDomain::PlayableUnitName
                        && epilogue_unit_name_source_index(&workset.record_id, *selector).is_some()
                    {
                        glyphs.len()
                    } else {
                        domains[&translated].maximum_entry_glyph_count
                    };
                    possible_domain_glyphs.extend(glyphs.iter().copied());
                    rendered_dynamic_glyph_upper_bound +=
                        *control_count * maximum_entry_glyph_count;
                }
            }
        }
        if has_translated_domain {
            translated_dynamic_page_count += 1;
        }
        if has_preserved_numeric {
            preserved_numeric_page_count += 1;
        }
        if has_translated_domain && has_preserved_numeric {
            mixed_dynamic_domain_page_count += 1;
        }
        maximum_possible_domain_glyph_count =
            maximum_possible_domain_glyph_count.max(possible_domain_glyphs.len());
        let rendered_target_glyph_upper_bound =
            target_glyphs.len() + rendered_dynamic_glyph_upper_bound;
        maximum_rendered_target_glyph_upper_bound =
            maximum_rendered_target_glyph_upper_bound.max(rendered_target_glyph_upper_bound);
        target_glyphs.extend(possible_domain_glyphs.iter().copied());
        let slot_demand = target_glyphs.len() + preserved_active_codes.len();
        maximum_augmented_workset_slot_demand =
            maximum_augmented_workset_slot_demand.max(slot_demand);
        augmented_worksets.push(GlyphWorkset {
            target_glyphs,
            preserved_active_codes,
            fixed_glyph_codes: BTreeMap::new(),
        });
        dynamic_glyphs_by_workset.push(possible_domain_glyphs);
        translated_dynamic_by_workset.push(has_translated_domain);
        preserved_numeric_by_workset.push(has_preserved_numeric);
    }

    // `{EC}` 생산 바이트를 페이지마다 다시 해석하지 않는다. 각 동적 글리프가
    // 나타날 수 있는 모든 페이지의 보존 코드를 먼저 모은 뒤, 그 어느 것과도
    // 충돌하지 않는 물리 코드를 하나씩 배정한다. 이후 페이지 packer가 이 배정을
    // 고정 조건으로 받으므로 생산자가 쓴 canonical 바이트가 곧 소비 바이트다.
    let forbidden_codes_by_glyph = forbidden_dynamic_codes_across_transition_lifetimes(
        &translated_dynamic_glyphs,
        &augmented_worksets,
        &dynamic_glyphs_by_workset,
        transition_lifetimes,
    )?;
    let canonical_dynamic_codes =
        assign_canonical_dynamic_codes(&forbidden_codes_by_glyph, &active_codes)?;
    for (workset, dynamic_glyphs) in augmented_worksets
        .iter_mut()
        .zip(&dynamic_glyphs_by_workset)
    {
        workset.fixed_glyph_codes = dynamic_glyphs
            .iter()
            .copied()
            .map(|glyph| (glyph, canonical_dynamic_codes[&glyph]))
            .collect();
        ensure!(
            workset
                .fixed_glyph_codes
                .values()
                .all(|code| !workset.preserved_active_codes.contains(code)),
            "dynamic dialogue canonical code collides with a preserved page code"
        );
    }

    let dynamic_control_count = dialogue
        .page_worksets
        .iter()
        .map(|workset| workset.dynamic_string_control_count)
        .sum::<usize>();
    ensure!(
        classified_control_count == dynamic_control_count,
        "dynamic dialogue classification lost EC controls"
    );
    let every_augmented_workset_fits =
        maximum_augmented_workset_slot_demand <= crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT;
    ensure!(
        every_augmented_workset_fits,
        "dynamic dialogue workset needs {maximum_augmented_workset_slot_demand} active slots but only {} exist",
        crate::font_slots::ACTIVE_HANGUL_SLOT_COUNT
    );

    Ok(DynamicDialogueInputPlan {
        augmented_worksets,
        dynamic_glyphs_by_workset,
        translated_dynamic_by_workset,
        preserved_numeric_by_workset,
        canonical_dynamic_codes,
        declared_domain_count: DynamicStringDomain::ALL.len(),
        translated_dynamic_page_count,
        preserved_numeric_page_count,
        translated_dynamic_glyph_count: translated_dynamic_glyphs.len(),
        combined_dialogue_glyph_count,
        maximum_possible_domain_glyph_count,
        maximum_augmented_workset_slot_demand,
        maximum_rendered_target_glyph_upper_bound,
        mixed_dynamic_domain_page_count,
        every_dynamic_control_classified: true,
        every_augmented_workset_fits,
    })
}

fn forbidden_dynamic_codes_across_transition_lifetimes(
    translated_dynamic_glyphs: &BTreeSet<char>,
    worksets: &[GlyphWorkset],
    dynamic_glyphs_by_workset: &[BTreeSet<char>],
    transition_lifetimes: &[TransitionLifetimeWorksets],
) -> Result<BTreeMap<char, BTreeSet<u8>>> {
    ensure!(
        worksets.len() == dynamic_glyphs_by_workset.len(),
        "dynamic dialogue lifetime code assignment lost page worksets"
    );
    let mut forbidden_codes_by_glyph = translated_dynamic_glyphs
        .iter()
        .copied()
        .map(|glyph| (glyph, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    let mut covered_worksets = BTreeSet::new();
    for lifetime in transition_lifetimes {
        ensure!(
            !lifetime.workset_indices.is_empty(),
            "dynamic dialogue transition lifetime has no visible page"
        );
        let mut lifetime_preserved_codes = BTreeSet::new();
        let mut lifetime_dynamic_glyphs = BTreeSet::new();
        for workset_index in &lifetime.workset_indices {
            ensure!(
                *workset_index < worksets.len() && covered_worksets.insert(*workset_index),
                "dynamic dialogue transition lifetimes overlap or leave their page domain"
            );
            lifetime_preserved_codes.extend(
                worksets[*workset_index]
                    .preserved_active_codes
                    .iter()
                    .copied(),
            );
            lifetime_dynamic_glyphs
                .extend(dynamic_glyphs_by_workset[*workset_index].iter().copied());
        }
        for glyph in lifetime_dynamic_glyphs {
            forbidden_codes_by_glyph
                .get_mut(&glyph)
                .with_context(|| {
                    format!("transition lifetime contains unknown dynamic glyph {glyph:?}")
                })?
                .extend(lifetime_preserved_codes.iter().copied());
        }
    }
    ensure!(
        covered_worksets.len() == worksets.len(),
        "dynamic dialogue transition lifetimes do not cover every visible page"
    );
    Ok(forbidden_codes_by_glyph)
}

fn assign_canonical_dynamic_codes(
    forbidden_codes_by_glyph: &BTreeMap<char, BTreeSet<u8>>,
    active_codes: &BTreeSet<u8>,
) -> Result<BTreeMap<char, u8>> {
    let candidates = forbidden_codes_by_glyph
        .iter()
        .map(|(glyph, forbidden)| {
            let allowed = active_codes
                .difference(forbidden)
                .copied()
                .collect::<BTreeSet<_>>();
            ensure!(
                !allowed.is_empty(),
                "dynamic dialogue glyph {glyph:?} has no code valid across its pages"
            );
            Ok((*glyph, allowed))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut glyph_order = candidates.keys().copied().collect::<Vec<_>>();
    glyph_order.sort_by_key(|glyph| (candidates[glyph].len(), *glyph));

    let mut glyph_by_code = BTreeMap::<u8, char>::new();
    for glyph in glyph_order {
        let mut visited_codes = BTreeSet::new();
        ensure!(
            assign_dynamic_glyph_code(glyph, &candidates, &mut glyph_by_code, &mut visited_codes),
            "dynamic dialogue glyphs have no injective code assignment across all pages"
        );
    }
    let assignments = glyph_by_code
        .into_iter()
        .map(|(code, glyph)| (glyph, code))
        .collect::<BTreeMap<_, _>>();
    ensure!(
        assignments.len() == candidates.len()
            && assignments
                .iter()
                .all(|(glyph, code)| candidates[glyph].contains(code)),
        "dynamic dialogue matching returned an invalid canonical assignment"
    );
    Ok(assignments)
}

fn assign_dynamic_glyph_code(
    glyph: char,
    candidates: &BTreeMap<char, BTreeSet<u8>>,
    glyph_by_code: &mut BTreeMap<u8, char>,
    visited_codes: &mut BTreeSet<u8>,
) -> bool {
    for code in &candidates[&glyph] {
        if !visited_codes.insert(*code) {
            continue;
        }
        let displaced = glyph_by_code.get(code).copied();
        if displaced.is_none_or(|other| {
            assign_dynamic_glyph_code(other, candidates, glyph_by_code, visited_codes)
        }) {
            glyph_by_code.insert(*code, glyph);
            return true;
        }
    }
    false
}

struct DomainGlyphs {
    glyphs: BTreeSet<char>,
    maximum_entry_glyph_count: usize,
}

fn domain_glyphs(entries: &[FixedTextPlannedEntry], table_id: &str) -> Result<DomainGlyphs> {
    let selected = entries
        .iter()
        .filter(|entry| entry.table_id == table_id)
        .collect::<Vec<_>>();
    ensure!(
        !selected.is_empty(),
        "dynamic dialogue domain {table_id} has no translated entries"
    );
    Ok(DomainGlyphs {
        glyphs: selected
            .iter()
            .flat_map(|entry| entry.unique_glyphs())
            .collect(),
        maximum_entry_glyph_count: selected
            .iter()
            .map(|entry| entry.unique_glyphs().len())
            .max()
            .unwrap_or(0),
    })
}

impl DynamicStringDomain {
    const ALL: [Self; 4] = [
        Self::PreservedNumeric,
        Self::ItemName,
        Self::PlayableUnitName,
        Self::LocationName,
    ];
}

/// 인물 후일담은 엔트리 하나가 아군 하나를 가리키고, 전부 같은 형태로 시작한다.
/// `{E2}{E9:05}{EC:00}` 뒤에 그 인물의 결말이 이어진다. 후일담 표는 53분기이고
/// 라우팅 표가 그중 하나로 합류하므로 항목을 낱낱이 세지 않고 규칙으로 둔다.
fn epilogue_entry_names_a_playable_unit(record_id: &str, selector: u8) -> bool {
    epilogue_unit_name_source_index(record_id, selector).is_some()
}

fn epilogue_unit_name_source_index(record_id: &str, selector: u8) -> Option<usize> {
    if selector != 0 {
        return None;
    }
    let Some((table, entry)) = record_id.rsplit_once(':') else {
        return None;
    };
    let entry = entry.parse::<usize>().unwrap_or(usize::MAX);
    let names_a_unit = match table {
        // 0번은 인물이 아니라 전사 장소를 넣는다. 그쪽은 지명으로 따로 결속돼 있다.
        "epilogue-dialogue" => (1..=53).contains(&entry),
        "epilogue-routing-dialogue" => (2..=53).contains(&entry),
        _ => false,
    };
    names_a_unit.then(|| entry - 1)
}

fn is_ending_character_epilogue_record(record_id: &str) -> bool {
    record_id.starts_with("epilogue-dialogue:")
        || record_id.starts_with("epilogue-routing-dialogue:")
}

fn possible_dynamic_glyphs(
    record_id: &str,
    selector: u8,
    domain: DynamicStringDomain,
    domains: &BTreeMap<DynamicStringDomain, DomainGlyphs>,
    unit_names: &[FixedTextPlannedEntry],
) -> Result<BTreeSet<char>> {
    if domain == DynamicStringDomain::PlayableUnitName
        && let Some(source_index) = epilogue_unit_name_source_index(record_id, selector)
    {
        let entry = unit_names
            .iter()
            .find(|entry| entry.table_id == "unit-names" && entry.source_index == source_index)
            .with_context(|| {
                format!(
                    "ending character epilogue record {record_id} lost unit-name source index {source_index}"
                )
            })?;
        return Ok(entry.unique_glyphs());
    }
    Ok(domains[&domain].glyphs.clone())
}

fn dynamic_string_domain(record_id: &str, selector: u8) -> Option<DynamicStringDomain> {
    let binding = (record_id, selector);
    if epilogue_entry_names_a_playable_unit(record_id, selector) {
        Some(DynamicStringDomain::PlayableUnitName)
    } else if ITEM_NAME_BINDINGS.contains(&binding) {
        Some(DynamicStringDomain::ItemName)
    } else if PLAYABLE_UNIT_NAME_BINDINGS.contains(&binding) {
        Some(DynamicStringDomain::PlayableUnitName)
    } else if LOCATION_NAME_BINDINGS.contains(&binding) {
        Some(DynamicStringDomain::LocationName)
    } else if PRESERVED_NUMERIC_BINDINGS.contains(&binding) {
        Some(DynamicStringDomain::PreservedNumeric)
    } else {
        None
    }
}

pub(super) fn classified_dynamic_string_bindings()
-> BTreeMap<(&'static str, u8), DynamicStringDomain> {
    let mut bindings = BTreeMap::new();
    for (domain, entries) in [
        (DynamicStringDomain::ItemName, ITEM_NAME_BINDINGS.as_slice()),
        (
            DynamicStringDomain::PlayableUnitName,
            PLAYABLE_UNIT_NAME_BINDINGS.as_slice(),
        ),
        (
            DynamicStringDomain::LocationName,
            LOCATION_NAME_BINDINGS.as_slice(),
        ),
        (
            DynamicStringDomain::PreservedNumeric,
            PRESERVED_NUMERIC_BINDINGS.as_slice(),
        ),
    ] {
        for binding in entries {
            assert!(
                bindings.insert(*binding, domain).is_none(),
                "duplicate dynamic dialogue binding {binding:?}"
            );
        }
    }
    for (table, entries) in [
        ("epilogue-dialogue", 1..=53usize),
        ("epilogue-routing-dialogue", 2..=53usize),
    ] {
        for entry in entries {
            let record_id: &'static str = Box::leak(format!("{table}:{entry:03}").into_boxed_str());
            assert!(
                bindings
                    .insert((record_id, 0), DynamicStringDomain::PlayableUnitName)
                    .is_none(),
                "duplicate epilogue dynamic dialogue binding {record_id}"
            );
        }
    }
    bindings
}

const ITEM_NAME_BINDINGS: [(&str, u8); 12] = [
    // 레코드 프리픽스 파서를 고치기 전에는 이 다섯 결속이 잘린 네 바이트 안에 있어
    // 분류표가 볼 수 없었다. 의사결정 57번을 따른다.
    ("shop-and-item-dialogue:001", 0),
    ("shop-and-item-dialogue:067", 0),
    ("shop-and-item-dialogue:069", 1),
    ("shop-and-item-dialogue:070", 0),
    ("shop-and-item-dialogue:072", 0),
    ("village-and-outro-dialogue:014", 0),
    ("village-and-outro-dialogue:021", 0),
    ("shop-and-item-dialogue:008", 0),
    ("shop-and-item-dialogue:025", 1),
    ("shop-and-item-dialogue:026", 1),
    ("shop-and-item-dialogue:027", 1),
    ("shop-and-item-dialogue:028", 1),
];

const PLAYABLE_UNIT_NAME_BINDINGS: [(&str, u8); 5] = [
    ("shop-and-item-dialogue:025", 0),
    ("shop-and-item-dialogue:026", 0),
    ("shop-and-item-dialogue:027", 0),
    ("shop-and-item-dialogue:027", 2),
    ("shop-and-item-dialogue:028", 0),
];

const LOCATION_NAME_BINDINGS: [(&str, u8); 1] = [("epilogue-dialogue:000", 1)];

const PRESERVED_NUMERIC_BINDINGS: [(&str, u8); 19] = [
    ("village-and-outro-dialogue:000", 0),
    ("village-and-outro-dialogue:004", 0),
    ("village-and-outro-dialogue:008", 0),
    ("village-and-outro-dialogue:017", 0),
    ("village-and-outro-dialogue:020", 0),
    ("shop-and-item-dialogue:015", 1),
    ("shop-and-item-dialogue:021", 1),
    ("shop-and-item-dialogue:029", 2),
    ("shop-and-item-dialogue:030", 2),
    ("shop-and-item-dialogue:031", 2),
    ("shop-and-item-dialogue:032", 2),
    ("shop-and-item-dialogue:033", 2),
    ("shop-and-item-dialogue:034", 2),
    ("shop-and-item-dialogue:035", 2),
    ("shop-and-item-dialogue:036", 2),
    ("shop-and-item-dialogue:037", 2),
    ("shop-and-item-dialogue:038", 2),
    ("shop-and-item-dialogue:040", 2),
    ("shop-and-item-dialogue:042", 0),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dialogue_assets::MainDialoguePageWorkset, text_inventory::FixedTextLogicalByte};

    fn fixed_entry(table_id: &str, source_index: usize, text: &str) -> FixedTextPlannedEntry {
        FixedTextPlannedEntry {
            id: format!("{table_id}:{source_index:03}"),
            table_id: table_id.to_string(),
            source_index,
            alias_indices: Vec::new(),
            file_offset: 0,
            source_storage_byte_count: text.chars().count(),
            review_complete: true,
            logical_bytes: text
                .chars()
                .map(FixedTextLogicalByte::TargetGlyph)
                .collect(),
        }
    }

    fn one_page_display(record_id: &str, selector: u8) -> MainDialogueDisplayPlan {
        MainDialogueDisplayPlan {
            canonical_record_count: 1,
            record_ids: vec![record_id.to_string()],
            page_worksets: vec![MainDialoguePageWorkset {
                record_id: record_id.to_string(),
                page_index: 0,
                target_glyphs: BTreeSet::from(['끝']),
                dynamic_string_selectors: BTreeSet::from([selector]),
                dynamic_string_selector_counts: BTreeMap::from([(selector, 1)]),
                dynamic_string_control_count: 1,
                source_reclaimable_active_codes: BTreeSet::new(),
                preserved_target_active_codes: BTreeSet::new(),
            }],
        }
    }

    #[test]
    fn unknown_dynamic_string_binding_fails_closed() {
        assert_eq!(
            dynamic_string_domain("shop-and-item-dialogue:025", 0),
            Some(DynamicStringDomain::PlayableUnitName)
        );
        assert_eq!(
            dynamic_string_domain("shop-and-item-dialogue:025", 1),
            Some(DynamicStringDomain::ItemName)
        );
        assert_eq!(
            dynamic_string_domain("shop-and-item-dialogue:008", 0),
            Some(DynamicStringDomain::ItemName)
        );
        assert_eq!(dynamic_string_domain("unknown", 0), None);
    }

    /// 인물 후일담은 항목마다 결속을 나열하지 않고 규칙으로 둔다. 규칙이 표를 벗어나
    /// 다른 표까지 삼키면 엉뚱한 레코드에 아군 이름이 들어가므로 경계를 확인한다.
    #[test]
    fn epilogue_unit_name_rule_covers_only_character_branches() {
        for (record_id, expected) in [
            // 0번은 인물이 아니라 전사 장소를 넣는다. 선택자도 0이 아니라 1이다.
            ("epilogue-dialogue:000", None),
            (
                "epilogue-dialogue:001",
                Some(DynamicStringDomain::PlayableUnitName),
            ),
            (
                "epilogue-dialogue:053",
                Some(DynamicStringDomain::PlayableUnitName),
            ),
            ("epilogue-dialogue:054", None),
            ("epilogue-routing-dialogue:001", None),
            (
                "epilogue-routing-dialogue:002",
                Some(DynamicStringDomain::PlayableUnitName),
            ),
            ("chapter-intro-dialogue:001", None),
        ] {
            assert_eq!(dynamic_string_domain(record_id, 0), expected, "{record_id}");
        }
        // 선택자 0만 이름을 받는다. 후일담의 다른 선택자는 규칙 밖이다.
        assert_eq!(dynamic_string_domain("epilogue-dialogue:001", 1), None);
        assert_eq!(
            dynamic_string_domain("epilogue-dialogue:000", 1),
            Some(DynamicStringDomain::LocationName)
        );
        assert_eq!(
            epilogue_unit_name_source_index("epilogue-dialogue:001", 0),
            Some(0)
        );
        assert_eq!(
            epilogue_unit_name_source_index("epilogue-routing-dialogue:053", 0),
            Some(52)
        );
    }

    #[test]
    fn ending_character_analysis_feeds_emitted_workset_constraints() {
        let preserved = ending_character_epilogue_preserved_active_codes();
        let plan = plan_dynamic_dialogue_inputs(
            &one_page_display("epilogue-dialogue:001", 0),
            &[fixed_entry("item-names", 0, "검")],
            &[
                fixed_entry("unit-names", 0, "마르스"),
                fixed_entry("unit-names", 1, "치키"),
            ],
            &[fixed_entry("location-names", 0, "아리티아")],
            &[TransitionLifetimeWorksets {
                record_indices: vec![0],
                workset_indices: vec![0],
            }],
        )
        .unwrap();
        let installed_workset = &plan.augmented_worksets[0];

        assert_eq!(
            preserved,
            BTreeSet::from([0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA])
        );
        assert_eq!(installed_workset.preserved_active_codes, preserved);
        assert!(
            installed_workset
                .target_glyphs
                .is_superset(&BTreeSet::from(['끝', '마', '르', '스']))
        );
        assert!(!installed_workset.target_glyphs.contains(&'키'));
        assert!(
            [0xF5, 0xF6, 0xF7, 0xF8, 0xF9, 0xFA]
                .into_iter()
                .all(|code| installed_workset.preserved_active_codes.contains(&code))
        );
        assert!(
            installed_workset
                .fixed_glyph_codes
                .values()
                .all(|code| !installed_workset.preserved_active_codes.contains(code))
        );
    }

    /// 분류표는 소비처마다 하나씩만 있어야 한다. 규칙과 나열이 겹치면 조립이 실패한다.
    #[test]
    fn every_classified_binding_is_unique() {
        let bindings = classified_dynamic_string_bindings();

        assert_eq!(
            bindings.len(),
            ITEM_NAME_BINDINGS.len()
                + PLAYABLE_UNIT_NAME_BINDINGS.len()
                + LOCATION_NAME_BINDINGS.len()
                + PRESERVED_NUMERIC_BINDINGS.len()
                + 53
                + 52
        );
    }

    #[test]
    fn canonical_matching_reserves_the_scarce_code_for_the_constrained_glyph() {
        let active = BTreeSet::from([1, 2]);
        let forbidden = BTreeMap::from([('가', BTreeSet::new()), ('나', BTreeSet::from([2]))]);

        let assignments = assign_canonical_dynamic_codes(&forbidden, &active).unwrap();

        assert_eq!(assignments[&'나'], 1);
        assert_eq!(assignments[&'가'], 2);
    }

    #[test]
    fn canonical_matching_fails_when_two_glyphs_have_only_one_code() {
        let active = BTreeSet::from([1]);
        let forbidden = BTreeMap::from([('가', BTreeSet::new()), ('나', BTreeSet::new())]);

        let error = assign_canonical_dynamic_codes(&forbidden, &active).unwrap_err();

        assert!(error.to_string().contains("no injective code assignment"));
    }

    #[test]
    fn dynamic_code_avoids_preserved_codes_on_every_page_in_its_visible_lifetime() {
        let dynamic_glyphs = BTreeSet::from(['훈']);
        let worksets = vec![
            GlyphWorkset {
                target_glyphs: dynamic_glyphs.clone(),
                preserved_active_codes: BTreeSet::new(),
                fixed_glyph_codes: BTreeMap::new(),
            },
            GlyphWorkset {
                target_glyphs: BTreeSet::new(),
                preserved_active_codes: BTreeSet::from([0x03]),
                fixed_glyph_codes: BTreeMap::new(),
            },
        ];
        let forbidden = forbidden_dynamic_codes_across_transition_lifetimes(
            &dynamic_glyphs,
            &worksets,
            &[dynamic_glyphs.clone(), BTreeSet::new()],
            &[TransitionLifetimeWorksets {
                record_indices: vec![0],
                workset_indices: vec![0, 1],
            }],
        )
        .unwrap();

        assert_eq!(forbidden[&'훈'], BTreeSet::from([0x03]));
        assert_ne!(
            assign_canonical_dynamic_codes(
                &forbidden,
                &active_hangul_codes().into_iter().collect()
            )
            .unwrap()[&'훈'],
            0x03
        );
    }
}
