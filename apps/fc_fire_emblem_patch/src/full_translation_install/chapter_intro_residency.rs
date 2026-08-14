use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};

use crate::{
    chapter_transition::{ChapterTitlePlan, bind_chapter_intro_lifetime_contexts},
    dialogue_assets::MainDialogueDisplayPlan,
    dialogue_inventory::{inspect_main_dialogue_graph, main_dialogue_transition_chain_record_ids},
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, active_hangul_codes},
    mapper165::battle_codebook_plan::GlyphWorkset,
    rom::Rom,
};

use super::resident_glyph_assignment::{assign_resident_glyph_codes, assignment_sha1};

const CHAPTER_COUNT: usize = 25;

pub(super) struct EncodedChapterTitle {
    pub(super) id: String,
    pub(super) file_offset: usize,
    pub(super) encoded_storage: Vec<u8>,
}

pub(super) struct ChapterIntroResidencyPlan {
    pub(super) augmented_worksets: Vec<GlyphWorkset>,
    pub(super) encoded_titles: Vec<EncodedChapterTitle>,
    pub(super) chapter_context_count: usize,
    pub(super) resident_workset_count: usize,
    pub(super) title_glyph_count: usize,
    pub(super) fixed_code_count: usize,
    pub(super) maximum_augmented_workset_slot_demand: usize,
    pub(super) fixed_assignment_sha1: String,
    /// 장 제목 저장소는 장 도입 대사와 엔딩 기록 화면이 함께 읽는다. 엔딩용 글꼴
    /// 페이지가 도입부와 다른 코드를 쓰면 저장소를 두 벌로 복제해야 하므로 실제
    /// 고정 코드를 다음 소비자 계획에도 넘긴다.
    pub(super) title_glyph_codes: BTreeMap<char, u8>,
}

pub(super) fn plan_chapter_intro_residency(
    rom: &Rom,
    display: &MainDialogueDisplayPlan,
    chapter_titles: &ChapterTitlePlan,
    dialogue_worksets: &[GlyphWorkset],
) -> Result<ChapterIntroResidencyPlan> {
    ensure!(
        display.page_worksets.len() == dialogue_worksets.len(),
        "chapter-intro residency lost dialogue page worksets"
    );
    let contexts = bind_chapter_intro_lifetime_contexts(rom)?;
    ensure!(
        contexts.len() == CHAPTER_COUNT
            && chapter_titles.entry_count == CHAPTER_COUNT
            && chapter_titles.translated_entry_count == CHAPTER_COUNT,
        "chapter-intro residency population changed"
    );
    let graph = inspect_main_dialogue_graph(rom.data())?;
    let mut workset_indices_by_record_id = BTreeMap::<&str, Vec<usize>>::new();
    for (index, workset) in display.page_worksets.iter().enumerate() {
        workset_indices_by_record_id
            .entry(workset.record_id.as_str())
            .or_default()
            .push(index);
    }

    let mut title_glyphs_by_workset = vec![BTreeSet::new(); dialogue_worksets.len()];
    let mut forbidden_codes_by_glyph = BTreeMap::<char, BTreeSet<u8>>::new();
    let mut preassigned_codes_by_glyph = BTreeMap::<char, BTreeSet<u8>>::new();
    let mut resident_workset_indices = BTreeSet::new();
    for context in &contexts {
        let title = chapter_titles.entry(context.chapter_index)?;
        let title_glyphs = title.unique_glyphs();
        ensure!(
            !title_glyphs.is_empty(),
            "{} has no target glyphs for chapter-intro residency",
            title.id
        );
        let record_ids = main_dialogue_transition_chain_record_ids(
            &graph,
            "chapter-intro-dialogue",
            context.canonical_entry_index,
        )?;
        for record_id in record_ids {
            let workset_indices = workset_indices_by_record_id
                .get(record_id.as_str())
                .with_context(|| {
                    format!("chapter-intro record {record_id} has no visible page worksets")
                })?;
            for workset_index in workset_indices {
                resident_workset_indices.insert(*workset_index);
                title_glyphs_by_workset[*workset_index].extend(title_glyphs.iter().copied());
                for glyph in &title_glyphs {
                    forbidden_codes_by_glyph.entry(*glyph).or_default().extend(
                        dialogue_worksets[*workset_index]
                            .preserved_active_codes
                            .iter()
                            .copied(),
                    );
                    for (fixed_glyph, fixed_code) in
                        &dialogue_worksets[*workset_index].fixed_glyph_codes
                    {
                        if fixed_glyph == glyph {
                            preassigned_codes_by_glyph
                                .entry(*glyph)
                                .or_default()
                                .insert(*fixed_code);
                        } else {
                            forbidden_codes_by_glyph
                                .entry(*glyph)
                                .or_default()
                                .insert(*fixed_code);
                        }
                    }
                }
            }
        }
    }

    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let title_glyph_codes = assign_resident_glyph_codes(
        "chapter-title residency",
        &forbidden_codes_by_glyph,
        &preassigned_codes_by_glyph,
        &active_codes,
    )?;
    let mut augmented_worksets = dialogue_worksets.to_vec();
    let mut maximum_augmented_workset_slot_demand = 0;
    for (workset, title_glyphs) in augmented_worksets.iter_mut().zip(&title_glyphs_by_workset) {
        for glyph in title_glyphs {
            let code = title_glyph_codes[glyph];
            ensure!(
                !workset.preserved_active_codes.contains(&code),
                "chapter-title glyph {glyph:?} uses a code preserved by its dialogue page"
            );
            workset.target_glyphs.insert(*glyph);
            if let Some(existing) = workset.fixed_glyph_codes.insert(*glyph, code) {
                ensure!(
                    existing == code,
                    "chapter-title glyph {glyph:?} changes its preassigned fixed code"
                );
            }
        }
        maximum_augmented_workset_slot_demand = maximum_augmented_workset_slot_demand
            .max(workset.target_glyphs.len() + workset.preserved_active_codes.len());
    }
    ensure!(
        maximum_augmented_workset_slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
        "chapter-intro page needs {maximum_augmented_workset_slot_demand} active slots but only {ACTIVE_HANGUL_SLOT_COUNT} exist"
    );

    let encoded_titles = chapter_titles
        .entries
        .iter()
        .map(|entry| {
            Ok(EncodedChapterTitle {
                id: entry.id.clone(),
                file_offset: entry.file_offset,
                encoded_storage: entry.encoded_storage_bytes(&title_glyph_codes)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        encoded_titles.len() == CHAPTER_COUNT,
        "chapter-intro residency lost encoded titles"
    );

    Ok(ChapterIntroResidencyPlan {
        augmented_worksets,
        encoded_titles,
        chapter_context_count: contexts.len(),
        resident_workset_count: resident_workset_indices.len(),
        title_glyph_count: title_glyph_codes.len(),
        fixed_code_count: title_glyph_codes.len(),
        maximum_augmented_workset_slot_demand,
        fixed_assignment_sha1: assignment_sha1(&title_glyph_codes),
        title_glyph_codes,
    })
}
