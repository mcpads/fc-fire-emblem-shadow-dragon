//! 실제 동시 표시 집합을 고정 페이지와 런타임 투영 대상으로 분류한다.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{
    font_slots::active_hangul_codes,
    semantic_translation::SemanticTranslationPlan,
    text_inventory::FixedTextLogicalByte,
    unit_ui_text::{command_menu_label_ids, summary_and_status_label_ids},
};

use super::{CodeOwner, ConsumerCodebookInputs, GlyphKey, Lifetime};

pub(super) fn build_lifetimes(inputs: &ConsumerCodebookInputs<'_>) -> Result<Vec<Lifetime>> {
    let fixed = |table_id: &str| {
        inputs
            .fixed
            .entries
            .iter()
            .filter(|entry| entry.table_id == table_id)
            .map(|entry| entry.logical_bytes.clone())
            .collect::<Vec<_>>()
    };
    let unit_names = inputs
        .unit_names
        .entries
        .iter()
        .map(|entry| entry.logical_bytes.clone())
        .collect::<Vec<_>>();
    let summary_labels = semantic_entries(inputs.unit_ui, &summary_and_status_label_ids())?;
    let command_labels = semantic_entries(inputs.unit_ui, &command_menu_label_ids())?;
    let item_action_labels = inputs
        .item_actions
        .entry_ids()
        .map(|id| {
            inputs
                .item_actions
                .entry_logical_bytes(id)
                .with_context(|| format!("item-action plan lost {id}"))
                .map(<[FixedTextLogicalByte]>::to_vec)
        })
        .collect::<Result<Vec<_>>>()?;
    let choices = inputs
        .choices
        .entries
        .iter()
        .map(|entry| entry.logical_bytes().to_vec())
        .collect::<Vec<_>>();
    let chapter_titles = inputs
        .chapter_titles
        .entries
        .iter()
        .map(|entry| entry.logical_bytes().to_vec())
        .collect::<Vec<_>>();
    let map_menu = inputs
        .map_menu
        .entries
        .iter()
        .map(|entry| entry.logical_bytes().to_vec())
        .collect::<Vec<_>>();

    let items = fixed("item-names");
    let classes = fixed("class-names");
    let enemies = fixed("enemy-names");
    ensure!(
        items.len() == 91 && classes.len() == 22 && enemies.len() == 69,
        "consumer codebook fixed-text populations changed"
    );

    let mut lifetimes = Vec::new();
    // 유닛명·클래스·아이템은 현재 선택에 따라 달라진다. 전체 카탈로그를 안정 코드로
    // 강제하면 269슬롯이 필요하므로 정적 색칠 대상이 아니다. KTX1 의미 셀을 실제
    // 선택된 항목에 맞춰 투영하는 런타임 소비자가 맡는다.
    let _runtime_projected_populations = (&unit_names, &classes, &enemies, &items, &summary_labels);
    lifetimes.push(lifetime(
        "unit_command_menu",
        "all_command_variants",
        vec!["unit_command_menu"],
        vec!["unit_ui_labels"],
        &[(CodeOwner::FixedUi, command_labels.as_slice())],
        true,
    ));
    let _runtime_projected_item_actions = item_action_labels;
    let mut map_lifetime = lifetime(
        "map_menu",
        "all_map_menu_entries",
        vec!["map_menu", "map_funds_summary"],
        vec!["map_menu_labels"],
        &[(CodeOwner::FixedUi, map_menu.as_slice())],
        true,
    );
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    map_lifetime.preserved_active_codes = active_codes
        .difference(&inputs.map_menu.source_reclaimable_active_codes)
        .copied()
        .collect();
    lifetimes.push(map_lifetime);
    let save_offer = vec![inputs.transitions.save_offer.logical_bytes.clone()];
    let mut save_offer_lifetime = lifetime(
        "chapter_save_offer",
        "save_question_and_both_choices",
        vec!["chapter_save_offer"],
        vec!["chapter_save_offer_label", "choice_labels"],
        &[
            (CodeOwner::FixedUi, save_offer.as_slice()),
            (CodeOwner::FixedUi, choices.as_slice()),
        ],
        true,
    );
    save_offer_lifetime
        .preserved_active_codes
        .extend(inputs.choices.preserved_active_codes.iter().copied());
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    save_offer_lifetime.preserved_active_codes.extend(
        inputs
            .transitions
            .save_offer
            .logical_bytes
            .iter()
            .filter_map(|byte| match byte {
                FixedTextLogicalByte::Encoded(code) if active_codes.contains(code) => Some(*code),
                FixedTextLogicalByte::Encoded(_) | FixedTextLogicalByte::TargetGlyph(_) => None,
            }),
    );
    lifetimes.push(save_offer_lifetime);
    let ending_label = vec![inputs.transitions.ending_record.logical_bytes.clone()];
    lifetimes.push(lifetime(
        "ending_chapter_record",
        "all_chapter_rows",
        vec!["ending_chapter_record_scroll"],
        vec!["chapter_titles", "ending_record_labels"],
        &[
            (CodeOwner::ChapterTitle, chapter_titles.as_slice()),
            (CodeOwner::FixedUi, ending_label.as_slice()),
        ],
        true,
    ));

    // 대사 동적 문자열은 정적 페이지를 만들지 않지만, 이미 저장 바이트가 된 187개
    // 코드를 같은 선색칠 계약으로 계속 검증한다.
    let dynamic_logical = inputs
        .dynamic_inputs
        .canonical_dynamic_codes()
        .keys()
        .copied()
        .map(FixedTextLogicalByte::TargetGlyph)
        .collect::<Vec<_>>();
    lifetimes.push(lifetime(
        "dialogue_dynamic_code_identity",
        "item_unit_location",
        vec!["dialogue_runtime"],
        vec!["item_names", "unit_names", "location_names"],
        &[(CodeOwner::DialogueDynamic, &[dynamic_logical])],
        false,
    ));
    Ok(lifetimes)
}

fn semantic_entries(
    plan: &SemanticTranslationPlan,
    ids: &[String],
) -> Result<Vec<Vec<FixedTextLogicalByte>>> {
    ids.iter()
        .map(|id| {
            plan.entry_logical_bytes(id)
                .with_context(|| format!("semantic plan lost {id}"))
                .map(<[FixedTextLogicalByte]>::to_vec)
        })
        .collect()
}

fn lifetime(
    id: &'static str,
    variant: &'static str,
    screen_roles: Vec<&'static str>,
    domain_ids: Vec<&'static str>,
    entry_groups: &[(CodeOwner, &[Vec<FixedTextLogicalByte>])],
    emit_static_page: bool,
) -> Lifetime {
    let mut lifetime = Lifetime {
        id,
        variant,
        screen_roles,
        domain_ids,
        target_glyphs: BTreeSet::new(),
        preserved_active_codes: BTreeSet::new(),
        emit_static_page,
    };
    for (owner, entries) in entry_groups {
        add_entries(&mut lifetime, *owner, entries);
    }
    lifetime
}

fn add_entries(lifetime: &mut Lifetime, owner: CodeOwner, entries: &[Vec<FixedTextLogicalByte>]) {
    for byte in entries.iter().flatten() {
        match byte {
            FixedTextLogicalByte::TargetGlyph(glyph) => {
                lifetime.target_glyphs.insert(GlyphKey {
                    owner,
                    glyph: *glyph,
                });
            }
            // `{ED}{19}`처럼 구조 바이트와 인수가 `Encoded`로 보존되기도 한다. 화면
            // 보존 코드는 소비자 원천 수명에서 별도로 넣어야 하며, 마크업 토큰을
            // 글꼴 사용으로 추정하면 실제로 보이지 않는 19까지 금지한다.
            FixedTextLogicalByte::Encoded(_) => {}
        }
    }
}

pub(super) fn forbidden_codes_by_glyph(lifetimes: &[Lifetime]) -> BTreeMap<GlyphKey, BTreeSet<u8>> {
    let mut forbidden = BTreeMap::<GlyphKey, BTreeSet<u8>>::new();
    for lifetime in lifetimes {
        for glyph in &lifetime.target_glyphs {
            forbidden
                .entry(*glyph)
                .or_default()
                .extend(&lifetime.preserved_active_codes);
        }
    }
    forbidden
}
