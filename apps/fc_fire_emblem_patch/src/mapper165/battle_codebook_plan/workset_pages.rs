use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

use anyhow::{Result, ensure};

use crate::{
    font_slots::{ACTIVE_HANGUL_SLOT_COUNT, FONT_PAGE_SIZE, FONT_TILE_SIZE, active_hangul_codes},
    sha1_hex,
};

use super::GlyphWorkset;

mod capacity_solver;

use capacity_solver::solve_page_capacity;

#[derive(Debug)]
pub(crate) struct GlyphWorksetPagePlan {
    pub(crate) page_assignments: Vec<BTreeMap<char, u8>>,
    pub(crate) workset_page_indices: Vec<usize>,
    pub(crate) glyph_count: usize,
    pub(crate) workset_count: usize,
    pub(crate) unique_workset_count: usize,
    pub(crate) maximum_workset_slot_demand: usize,
    pub(crate) maximum_page_slot_demand: usize,
    pub(crate) packing_sha1: String,
    pub(crate) page_assignment_sha1: String,
    pub(crate) greedy_page_count: usize,
    pub(crate) packing_strategy: &'static str,
    pub(crate) constraint_solver_version: Option<String>,
    pub(crate) constraint_solver_timeout_seconds: Option<u64>,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct WorksetSignature {
    target_glyphs: Vec<char>,
    preserved_codes: Vec<u8>,
    fixed_glyph_codes: Vec<(char, u8)>,
}

struct WorksetDemand {
    signature: WorksetSignature,
    original_indices: Vec<usize>,
}

#[derive(Clone, Default, Eq, PartialEq)]
struct FontPageDemand {
    target_glyphs: BTreeSet<char>,
    preserved_codes: BTreeSet<u8>,
    fixed_glyph_codes: BTreeMap<char, u8>,
}

impl FontPageDemand {
    fn slot_demand(&self) -> usize {
        self.target_glyphs.len() + self.preserved_codes.len()
    }

    fn merged(&self, workset: &WorksetSignature) -> Option<Self> {
        let mut merged = self.clone();
        merged
            .target_glyphs
            .extend(workset.target_glyphs.iter().copied());
        merged
            .preserved_codes
            .extend(workset.preserved_codes.iter().copied());
        for (glyph, code) in &workset.fixed_glyph_codes {
            if merged
                .fixed_glyph_codes
                .get(glyph)
                .is_some_and(|existing| existing != code)
                || merged
                    .fixed_glyph_codes
                    .iter()
                    .any(|(existing_glyph, existing_code)| {
                        existing_glyph != glyph && existing_code == code
                    })
            {
                return None;
            }
            merged.fixed_glyph_codes.insert(*glyph, *code);
        }
        (merged.slot_demand() <= ACTIVE_HANGUL_SLOT_COUNT
            && merged
                .fixed_glyph_codes
                .values()
                .all(|code| !merged.preserved_codes.contains(code)))
        .then_some(merged)
    }
}

pub(crate) fn plan_glyph_workset_pages(
    worksets: &[GlyphWorkset],
    maximum_page_count: usize,
) -> Result<GlyphWorksetPagePlan> {
    ensure!(!worksets.is_empty(), "glyph page pool has no worksets");
    ensure!(
        maximum_page_count > 0,
        "glyph page pool has no physical CHR page capacity"
    );
    let active_codes = active_hangul_codes().into_iter().collect::<BTreeSet<_>>();
    let mut unique = BTreeMap::<WorksetSignature, Vec<usize>>::new();
    let mut all_glyphs = BTreeSet::new();
    let mut maximum_workset_slot_demand = 0;
    for (index, workset) in worksets.iter().enumerate() {
        ensure!(
            workset.preserved_active_codes.is_subset(&active_codes),
            "glyph page workset preserves a reserved font code"
        );
        ensure!(
            workset
                .fixed_glyph_codes
                .keys()
                .all(|glyph| workset.target_glyphs.contains(glyph)),
            "glyph page workset fixes a code for a glyph outside its target set"
        );
        let fixed_codes = workset
            .fixed_glyph_codes
            .values()
            .copied()
            .collect::<BTreeSet<_>>();
        ensure!(
            fixed_codes.len() == workset.fixed_glyph_codes.len(),
            "glyph page workset assigns one fixed code to multiple glyphs"
        );
        ensure!(
            fixed_codes.is_subset(&active_codes),
            "glyph page workset fixes a reserved font code"
        );
        ensure!(
            fixed_codes.is_disjoint(&workset.preserved_active_codes),
            "glyph page workset fixes a code that the same page preserves"
        );
        let signature = WorksetSignature {
            target_glyphs: workset.target_glyphs.iter().copied().collect(),
            preserved_codes: workset.preserved_active_codes.iter().copied().collect(),
            fixed_glyph_codes: workset
                .fixed_glyph_codes
                .iter()
                .map(|(glyph, code)| (*glyph, *code))
                .collect(),
        };
        let slot_demand = signature.target_glyphs.len() + signature.preserved_codes.len();
        ensure!(
            slot_demand <= ACTIVE_HANGUL_SLOT_COUNT,
            "glyph page workset needs {slot_demand} active slots but only {} exist",
            ACTIVE_HANGUL_SLOT_COUNT
        );
        maximum_workset_slot_demand = maximum_workset_slot_demand.max(slot_demand);
        all_glyphs.extend(workset.target_glyphs.iter().copied());
        unique.entry(signature).or_default().push(index);
    }
    let unique_workset_count = unique.len();
    let mut demands = unique
        .into_iter()
        .map(|(signature, original_indices)| WorksetDemand {
            signature,
            original_indices,
        })
        .collect::<Vec<_>>();
    demands.sort_by(|left, right| {
        workset_slot_demand(&right.signature)
            .cmp(&workset_slot_demand(&left.signature))
            .then_with(|| {
                right
                    .signature
                    .target_glyphs
                    .len()
                    .cmp(&left.signature.target_glyphs.len())
            })
            .then_with(|| left.signature.cmp(&right.signature))
    });

    let (greedy_pages, greedy_unique_page_indices) = greedy_pack(&demands);
    let greedy_page_count = greedy_pages.len();
    let (unique_page_indices, packing_strategy, constraint_solver_version, solver_timeout) =
        if greedy_page_count <= maximum_page_count {
            (
                greedy_unique_page_indices,
                "deterministic best-fit decreasing set packing",
                None,
                None,
            )
        } else {
            let solved = solve_page_capacity(&demands, maximum_page_count)?;
            (
                solved.page_indices,
                solved.strategy,
                Some(solved.solver_version),
                Some(solved.timeout_seconds),
            )
        };
    let pages = build_pages(&demands, &unique_page_indices)?;
    let mut workset_page_indices = vec![usize::MAX; worksets.len()];
    for (demand, page_index) in demands.iter().zip(&unique_page_indices) {
        for original_index in &demand.original_indices {
            workset_page_indices[*original_index] = *page_index;
        }
    }
    ensure!(
        pages.len() <= maximum_page_count,
        "glyph page capacity solver returned {} pages for a {maximum_page_count}-page ceiling",
        pages.len()
    );
    ensure!(
        workset_page_indices
            .iter()
            .all(|index| *index < pages.len()),
        "glyph page packing lost a workset assignment"
    );

    let page_assignments = pages
        .iter()
        .map(|page| {
            let fixed_codes = page
                .fixed_glyph_codes
                .values()
                .copied()
                .collect::<BTreeSet<_>>();
            let available_codes = active_codes
                .difference(&page.preserved_codes)
                .filter(|code| !fixed_codes.contains(code))
                .copied()
                .collect::<Vec<_>>();
            ensure!(
                page.target_glyphs.len() - page.fixed_glyph_codes.len() <= available_codes.len(),
                "glyph page lost capacity after packing"
            );
            let mut assignments = page.fixed_glyph_codes.clone();
            assignments.extend(
                page.target_glyphs
                    .iter()
                    .filter(|glyph| !page.fixed_glyph_codes.contains_key(glyph))
                    .copied()
                    .zip(available_codes),
            );
            ensure!(
                assignments.len() == page.target_glyphs.len(),
                "glyph page assignment lost target glyphs"
            );
            Ok(assignments)
        })
        .collect::<Result<Vec<_>>>()?;
    let maximum_page_slot_demand = pages
        .iter()
        .map(FontPageDemand::slot_demand)
        .max()
        .unwrap_or(0);

    Ok(GlyphWorksetPagePlan {
        page_assignment_sha1: page_assignment_sha1(&page_assignments),
        packing_sha1: packing_sha1(&pages, &workset_page_indices),
        page_assignments,
        workset_page_indices,
        glyph_count: all_glyphs.len(),
        workset_count: worksets.len(),
        unique_workset_count,
        maximum_workset_slot_demand,
        maximum_page_slot_demand,
        greedy_page_count,
        packing_strategy,
        constraint_solver_version,
        constraint_solver_timeout_seconds: solver_timeout,
    })
}

pub(crate) fn plan_glyph_workset_page_upper_bound(
    worksets: &[GlyphWorkset],
) -> Result<GlyphWorksetPagePlan> {
    plan_glyph_workset_pages(worksets, usize::MAX)
}

/// 작업집합의 보존 계약이 실제로 방출한 4 KiB 페이지까지 이어졌는지 확인한다.
///
/// 분석 보고서의 보존 코드와 페이지 배치기가 서로 다른 입력을 소비하면 둘 다 개별적으로
/// 성공할 수 있다. 따라서 최종 작업집합→페이지 색인→방출 바이트를 다시 따라가며, 각 보호
/// 타일이 원본 글꼴 페이지와 바이트 단위로 같은지 확인한다.
pub(crate) fn verify_glyph_workset_font_page_pack(
    source_page: &[u8],
    worksets: &[GlyphWorkset],
    plan: &GlyphWorksetPagePlan,
    page_pack: &[u8],
) -> Result<()> {
    ensure!(
        source_page.len() == FONT_PAGE_SIZE,
        "glyph workset preservation source page is not 4 KiB"
    );
    ensure!(
        worksets.len() == plan.workset_page_indices.len(),
        "glyph workset preservation lost a workset-to-page route"
    );
    ensure!(
        page_pack.len() == plan.page_assignments.len() * FONT_PAGE_SIZE,
        "glyph workset preservation page pack length changed"
    );

    for (workset_index, workset) in worksets.iter().enumerate() {
        let page_index = plan.workset_page_indices[workset_index];
        let assignments = plan
            .page_assignments
            .get(page_index)
            .ok_or_else(|| anyhow::anyhow!("glyph workset {workset_index} selects no page"))?;
        ensure!(
            assignments
                .values()
                .all(|code| !workset.preserved_active_codes.contains(code)),
            "glyph workset {workset_index} assigns a Hangul glyph to a preserved screen code"
        );
        let page_start = page_index * FONT_PAGE_SIZE;
        let page = &page_pack[page_start..page_start + FONT_PAGE_SIZE];
        for code in &workset.preserved_active_codes {
            let tile_start = usize::from(*code) * FONT_TILE_SIZE;
            ensure!(
                page[tile_start..tile_start + FONT_TILE_SIZE]
                    == source_page[tile_start..tile_start + FONT_TILE_SIZE],
                "glyph workset {workset_index} changed preserved screen tile {code:02X} in emitted page {page_index}"
            );
        }
    }
    Ok(())
}

fn greedy_pack(demands: &[WorksetDemand]) -> (Vec<FontPageDemand>, Vec<usize>) {
    let mut pages = Vec::<FontPageDemand>::new();
    let mut page_indices = Vec::with_capacity(demands.len());
    for demand in demands {
        let selected = pages
            .iter()
            .enumerate()
            .filter_map(|(page_index, page)| {
                page.merged(&demand.signature).map(|merged| {
                    let added_slots = merged.slot_demand() - page.slot_demand();
                    let shared_glyphs = demand
                        .signature
                        .target_glyphs
                        .iter()
                        .filter(|glyph| page.target_glyphs.contains(glyph))
                        .count();
                    (page_index, merged, added_slots, shared_glyphs)
                })
            })
            .min_by(compare_page_choices);
        let page_index = if let Some((page_index, merged, _, _)) = selected {
            pages[page_index] = merged;
            page_index
        } else {
            pages.push(
                FontPageDemand::default()
                    .merged(&demand.signature)
                    .expect("validated workset must fit an empty font page"),
            );
            pages.len() - 1
        };
        page_indices.push(page_index);
    }
    (pages, page_indices)
}

fn build_pages(demands: &[WorksetDemand], page_indices: &[usize]) -> Result<Vec<FontPageDemand>> {
    ensure!(
        demands.len() == page_indices.len(),
        "glyph page solver assignment count changed"
    );
    let page_count = page_indices
        .iter()
        .copied()
        .max()
        .map_or(0, |page| page + 1);
    let mut pages = vec![FontPageDemand::default(); page_count];
    for (demand, page_index) in demands.iter().zip(page_indices) {
        let merged = pages[*page_index]
            .merged(&demand.signature)
            .ok_or_else(|| anyhow::anyhow!("glyph page solver emitted an over-capacity page"))?;
        pages[*page_index] = merged;
    }
    ensure!(
        pages.iter().all(|page| !page.target_glyphs.is_empty()),
        "glyph page solver left a gap in physical page numbering"
    );
    Ok(pages)
}

fn workset_slot_demand(workset: &WorksetSignature) -> usize {
    workset.target_glyphs.len() + workset.preserved_codes.len()
}

fn worksets_can_share_page(left: &WorksetSignature, right: &WorksetSignature) -> bool {
    FontPageDemand::default()
        .merged(left)
        .and_then(|page| page.merged(right))
        .is_some()
}

fn compare_page_choices(
    left: &(usize, FontPageDemand, usize, usize),
    right: &(usize, FontPageDemand, usize, usize),
) -> Ordering {
    let (left_index, left_page, left_added, left_shared) = left;
    let (right_index, right_page, right_added, right_shared) = right;
    left_added
        .cmp(right_added)
        .then_with(|| right_shared.cmp(left_shared))
        .then_with(|| right_page.slot_demand().cmp(&left_page.slot_demand()))
        .then_with(|| left_index.cmp(right_index))
}

fn packing_sha1(pages: &[FontPageDemand], workset_page_indices: &[usize]) -> String {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(pages.len() as u64).to_le_bytes());
    for page in pages {
        bytes.extend_from_slice(&(page.target_glyphs.len() as u64).to_le_bytes());
        for glyph in &page.target_glyphs {
            bytes.extend_from_slice(&u32::from(*glyph).to_le_bytes());
        }
        bytes.extend_from_slice(&(page.preserved_codes.len() as u64).to_le_bytes());
        bytes.extend(page.preserved_codes.iter());
        bytes.extend_from_slice(&(page.fixed_glyph_codes.len() as u64).to_le_bytes());
        for (glyph, code) in &page.fixed_glyph_codes {
            bytes.extend_from_slice(&u32::from(*glyph).to_le_bytes());
            bytes.push(*code);
        }
    }
    bytes.extend_from_slice(&(workset_page_indices.len() as u64).to_le_bytes());
    for page_index in workset_page_indices {
        bytes.extend_from_slice(&(*page_index as u64).to_le_bytes());
    }
    sha1_hex(&bytes)
}

fn page_assignment_sha1(assignments: &[BTreeMap<char, u8>]) -> String {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(assignments.len() as u64).to_le_bytes());
    for page in assignments {
        bytes.extend_from_slice(&(page.len() as u64).to_le_bytes());
        for (glyph, code) in page {
            bytes.extend_from_slice(&u32::from(*glyph).to_le_bytes());
            bytes.push(*code);
        }
    }
    sha1_hex(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workset(target_glyphs: &str, preserved_codes: &[u8]) -> GlyphWorkset {
        GlyphWorkset {
            target_glyphs: target_glyphs.chars().collect(),
            preserved_active_codes: preserved_codes.iter().copied().collect(),
            fixed_glyph_codes: BTreeMap::new(),
        }
    }

    fn fixed_workset(target_glyphs: &str, assignments: &[(char, u8)]) -> GlyphWorkset {
        GlyphWorkset {
            target_glyphs: target_glyphs.chars().collect(),
            preserved_active_codes: BTreeSet::new(),
            fixed_glyph_codes: assignments.iter().copied().collect(),
        }
    }

    #[test]
    fn compatible_worksets_share_one_page_and_one_page_local_codebook() {
        let active = active_hangul_codes();
        let plan = plan_glyph_workset_pages(
            &[workset("가나", &[active[0]]), workset("나다", &[active[1]])],
            1,
        )
        .unwrap();

        assert_eq!(plan.page_assignments.len(), 1);
        assert_eq!(plan.workset_page_indices, vec![0, 0]);
        assert_eq!(plan.page_assignments[0].len(), 3);
        assert!(
            plan.page_assignments[0]
                .values()
                .all(|code| ![active[0], active[1]].contains(code))
        );
    }

    #[test]
    fn combined_preservation_and_glyph_union_can_force_separate_pages() {
        let active = active_hangul_codes();
        let first_preserved = active[..ACTIVE_HANGUL_SLOT_COUNT - 1].to_vec();
        let second_preserved = active[1..].to_vec();
        let plan = plan_glyph_workset_pages(
            &[
                workset("가", &first_preserved),
                workset("나", &second_preserved),
            ],
            2,
        )
        .unwrap();

        assert_eq!(plan.page_assignments.len(), 2);
        assert_ne!(plan.workset_page_indices[0], plan.workset_page_indices[1]);
    }

    #[test]
    fn one_visible_workset_cannot_exceed_the_active_slot_capacity() {
        let glyphs = (0..=ACTIVE_HANGUL_SLOT_COUNT)
            .map(|index| char::from_u32(0xAC00 + index as u32).unwrap())
            .collect::<String>();

        let error = plan_glyph_workset_pages(&[workset(&glyphs, &[])], 1).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("active slots but only 210 exist")
        );
    }

    #[test]
    fn duplicate_worksets_share_the_same_page_assignment() {
        let plan =
            plan_glyph_workset_pages(&[workset("가나", &[]), workset("가나", &[])], 1).unwrap();

        assert_eq!(plan.unique_workset_count, 1);
        assert_eq!(plan.workset_page_indices, vec![0, 0]);
    }

    #[test]
    fn page_plan_is_deterministic() {
        let worksets = [workset("가나", &[]), workset("나다", &[])];

        let first = plan_glyph_workset_pages(&worksets, 1).unwrap();
        let second = plan_glyph_workset_pages(&worksets, 1).unwrap();

        assert_eq!(first.packing_sha1, second.packing_sha1);
        assert_eq!(first.page_assignment_sha1, second.page_assignment_sha1);
        assert_eq!(first.workset_page_indices, second.workset_page_indices);
        assert_eq!(first.page_assignments, second.page_assignments);
    }

    #[test]
    fn fixed_glyph_keeps_its_code_in_the_emitted_page_assignment() {
        let code = active_hangul_codes()[17];

        let plan = plan_glyph_workset_pages(&[fixed_workset("가나", &[('가', code)])], 1).unwrap();

        assert_eq!(plan.page_assignments[0][&'가'], code);
        assert_ne!(plan.page_assignments[0][&'나'], code);
    }

    #[test]
    fn incompatible_fixed_codes_force_distinct_pages() {
        let code = active_hangul_codes()[17];
        let plan = plan_glyph_workset_pages(
            &[
                fixed_workset("가", &[('가', code)]),
                fixed_workset("나", &[('나', code)]),
            ],
            2,
        )
        .unwrap();

        assert_ne!(plan.workset_page_indices[0], plan.workset_page_indices[1]);
    }

    #[test]
    fn fixed_code_cannot_share_a_page_that_preserves_it() {
        let code = active_hangul_codes()[17];
        let plan = plan_glyph_workset_pages(
            &[fixed_workset("가", &[('가', code)]), workset("나", &[code])],
            2,
        )
        .unwrap();

        assert_ne!(plan.workset_page_indices[0], plan.workset_page_indices[1]);
    }

    #[test]
    fn emitted_page_pack_keeps_preserved_tile_bytes_unchanged() {
        let code = active_hangul_codes()[17];
        let worksets = [workset("가나", &[code])];
        let plan = plan_glyph_workset_pages(&worksets, 1).unwrap();
        let source_page = (0..FONT_PAGE_SIZE)
            .map(|offset| (offset & 0xFF) as u8)
            .collect::<Vec<_>>();
        let mut page_pack =
            super::super::build_glyph_workset_font_page_pack(&source_page, &plan).unwrap();

        verify_glyph_workset_font_page_pack(&source_page, &worksets, &plan, &page_pack).unwrap();

        page_pack[usize::from(code) * FONT_TILE_SIZE] ^= 0xFF;
        let error = verify_glyph_workset_font_page_pack(&source_page, &worksets, &plan, &page_pack)
            .unwrap_err();
        assert!(error.to_string().contains("changed preserved screen tile"));
    }
}
