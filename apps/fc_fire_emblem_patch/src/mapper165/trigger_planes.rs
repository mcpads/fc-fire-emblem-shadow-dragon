use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{
    rom::{EXPECTED_SOURCE_SHA1, Rom},
    sha1_hex,
};

const CHR_PAGE_SIZE: usize = 0x1000;
const FD_TILE_HIGH_PLANE_OFFSET: usize = 0x0FD8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum PatternWindow {
    Left,
    Right,
}

impl PatternWindow {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Left => "ppu_0000",
            Self::Right => "ppu_1000",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ObservedVariantPair {
    pub(super) pattern_window: PatternWindow,
    pub(super) fd_source_page: u8,
    pub(super) fe_source_page: u8,
    pub(super) required_high_plane: [u8; 8],
}

#[derive(Debug, Clone, Copy)]
struct ObservedChrPair {
    screen_role: &'static str,
    pattern_window: PatternWindow,
    fd_source_page: u8,
    fe_source_page: u8,
}

const OBSERVED_CHR_PAIRS: &[ObservedChrPair] = &[
    observed("title", PatternWindow::Left, 0x14, 0x14),
    observed("title", PatternWindow::Right, 0x00, 0x14),
    observed("new_game_choice", PatternWindow::Left, 0x1A, 0x1A),
    observed("new_game_choice", PatternWindow::Right, 0x00, 0x00),
    observed("intro_terrain", PatternWindow::Left, 0x1A, 0x1A),
    observed("intro_terrain", PatternWindow::Right, 0x15, 0x15),
    observed("intro_dialogue", PatternWindow::Left, 0x07, 0x07),
    observed("intro_dialogue", PatternWindow::Right, 0x00, 0x18),
    observed("later_intro_dialogue", PatternWindow::Left, 0x11, 0x11),
    observed("later_intro_dialogue", PatternWindow::Right, 0x00, 0x18),
    observed("map_idle", PatternWindow::Left, 0x1A, 0x1A),
    observed("map_idle", PatternWindow::Right, 0x15, 0x15),
    observed("unit_status", PatternWindow::Left, 0x13, 0x13),
    observed("unit_status", PatternWindow::Right, 0x00, 0x18),
    observed("map_menu", PatternWindow::Left, 0x1A, 0x1A),
    observed("map_menu", PatternWindow::Right, 0x00, 0x19),
    observed("unit_roster", PatternWindow::Left, 0x18, 0x18),
    observed("unit_roster", PatternWindow::Right, 0x00, 0x19),
];

const fn observed(
    screen_role: &'static str,
    pattern_window: PatternWindow,
    fd_source_page: u8,
    fe_source_page: u8,
) -> ObservedChrPair {
    ObservedChrPair {
        screen_role,
        pattern_window,
        fd_source_page,
        fe_source_page,
    }
}

#[derive(Debug, Serialize)]
struct TriggerPlaneReport {
    schema: u32,
    source_sha1: &'static str,
    chr_4k_page_count: usize,
    trigger_timing: TriggerTiming,
    observed_screen_count: usize,
    unique_pair_count: usize,
    pair_compatibility: Vec<PairCompatibility>,
    fd_page_domains: Vec<FdPageDomain>,
    required_variant_pages: Vec<VariantRequirement>,
    required_variant_page_count: usize,
    pair_aware_selector_required: bool,
    all_observed_pairs_exact_without_variants: bool,
    unresolved_boundaries: Vec<&'static str>,
    release_eligible: bool,
}

#[derive(Debug, Serialize)]
struct TriggerTiming {
    mmc4_fd_trigger_addresses: [&'static str; 2],
    mapper165_fd_trigger_addresses: [&'static str; 2],
    compared_bytes: &'static str,
    preservation_rule: &'static str,
}

#[derive(Debug, Serialize)]
struct PairCompatibility {
    pattern_window: &'static str,
    fd_source_page: u8,
    fe_source_page: u8,
    screen_roles: Vec<&'static str>,
    fd_high_plane_sha1: String,
    required_fe_high_plane_sha1: String,
    classification: &'static str,
}

#[derive(Debug, Serialize)]
struct FdPageDomain {
    fd_source_page: u8,
    natural_high_plane_sha1: String,
    required_high_plane_sha1s: Vec<String>,
    unique_required_high_plane_count: usize,
    global_page_rewrite_sufficient: bool,
    pair_aware_selector_required: bool,
}

#[derive(Debug, Serialize)]
struct VariantRequirement {
    fd_source_page: u8,
    required_high_plane_sha1: String,
    compatible_fe_source_pages: Vec<u8>,
}

pub struct TriggerPlaneSummary {
    pub report_sha1: String,
    pub observed_screen_count: usize,
    pub unique_pair_count: usize,
    pub required_variant_page_count: usize,
    pub pair_aware_selector_required: bool,
}

pub fn analyze_mapper165_trigger_planes(
    source_path: &Path,
    report_path: &Path,
) -> Result<TriggerPlaneSummary> {
    let source_rom = Rom::from_path(source_path)?;
    source_rom.verify_supported_japanese()?;
    let report = analyze_observations(source_rom.chr(), OBSERVED_CHR_PAIRS)?;
    let report_bytes =
        serde_json::to_vec_pretty(&report).context("serialize mapper 165 trigger-plane report")?;
    let report_sha1 = sha1_hex(&report_bytes);

    if let Some(parent) = report_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    fs::write(report_path, report_bytes)
        .with_context(|| format!("write {}", report_path.display()))?;

    Ok(TriggerPlaneSummary {
        report_sha1,
        observed_screen_count: report.observed_screen_count,
        unique_pair_count: report.unique_pair_count,
        required_variant_page_count: report.required_variant_page_count,
        pair_aware_selector_required: report.pair_aware_selector_required,
    })
}

pub(super) fn observed_variant_pairs(chr: &[u8]) -> Result<Vec<ObservedVariantPair>> {
    ensure!(
        chr.len().is_multiple_of(CHR_PAGE_SIZE),
        "CHR length is not a whole number of 4 KiB pages"
    );
    let chr_page_count = chr.len() / CHR_PAGE_SIZE;
    let unique_pairs = OBSERVED_CHR_PAIRS
        .iter()
        .map(|pair| {
            (
                pair.pattern_window,
                pair.fd_source_page,
                pair.fe_source_page,
            )
        })
        .collect::<BTreeSet<_>>();

    unique_pairs
        .into_iter()
        .filter_map(|(pattern_window, fd_source_page, fe_source_page)| {
            let result = (|| {
                ensure!(
                    (fd_source_page as usize) < chr_page_count,
                    "FD source page {fd_source_page:02X} is outside CHR"
                );
                ensure!(
                    (fe_source_page as usize) < chr_page_count,
                    "FE source page {fe_source_page:02X} is outside CHR"
                );
                let fd_high_plane = fd_tile_high_plane(chr, fd_source_page)?;
                let required_high_plane = fd_tile_high_plane(chr, fe_source_page)?;
                Ok(
                    (fd_high_plane != required_high_plane).then_some(ObservedVariantPair {
                        pattern_window,
                        fd_source_page,
                        fe_source_page,
                        required_high_plane,
                    }),
                )
            })();
            match result {
                Ok(Some(pair)) => Some(Ok(pair)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            }
        })
        .collect()
}

fn analyze_observations(
    chr: &[u8],
    observations: &[ObservedChrPair],
) -> Result<TriggerPlaneReport> {
    ensure!(
        chr.len().is_multiple_of(CHR_PAGE_SIZE),
        "CHR length is not a whole number of 4 KiB pages"
    );
    let chr_page_count = chr.len() / CHR_PAGE_SIZE;
    ensure!(!observations.is_empty(), "no observed CHR pairs supplied");

    let mut pair_screens = BTreeMap::<(PatternWindow, u8, u8), BTreeSet<&'static str>>::new();
    for observation in observations {
        ensure!(
            (observation.fd_source_page as usize) < chr_page_count,
            "FD source page {:02X} is outside CHR",
            observation.fd_source_page
        );
        ensure!(
            (observation.fe_source_page as usize) < chr_page_count,
            "FE source page {:02X} is outside CHR",
            observation.fe_source_page
        );
        pair_screens
            .entry((
                observation.pattern_window,
                observation.fd_source_page,
                observation.fe_source_page,
            ))
            .or_default()
            .insert(observation.screen_role);
    }

    let mut requirements_by_fd = BTreeMap::<u8, BTreeSet<[u8; 8]>>::new();
    let mut variant_fe_pages = BTreeMap::<(u8, [u8; 8]), BTreeSet<u8>>::new();
    let mut pair_compatibility = Vec::with_capacity(pair_screens.len());

    for ((window, fd_page, fe_page), screens) in pair_screens {
        let fd_high_plane = fd_tile_high_plane(chr, fd_page)?;
        let required_high_plane = fd_tile_high_plane(chr, fe_page)?;
        requirements_by_fd
            .entry(fd_page)
            .or_default()
            .insert(required_high_plane);
        let classification = if fd_high_plane == required_high_plane {
            "exact"
        } else {
            variant_fe_pages
                .entry((fd_page, required_high_plane))
                .or_default()
                .insert(fe_page);
            "variant_required"
        };
        pair_compatibility.push(PairCompatibility {
            pattern_window: window.label(),
            fd_source_page: fd_page,
            fe_source_page: fe_page,
            screen_roles: screens.into_iter().collect(),
            fd_high_plane_sha1: sha1_hex(&fd_high_plane),
            required_fe_high_plane_sha1: sha1_hex(&required_high_plane),
            classification,
        });
    }

    let fd_page_domains = requirements_by_fd
        .iter()
        .map(|(fd_page, requirements)| {
            let unique_required_high_plane_count = requirements.len();
            Ok(FdPageDomain {
                fd_source_page: *fd_page,
                natural_high_plane_sha1: sha1_hex(&fd_tile_high_plane(chr, *fd_page)?),
                required_high_plane_sha1s: requirements
                    .iter()
                    .map(|plane| sha1_hex(plane))
                    .collect(),
                unique_required_high_plane_count,
                global_page_rewrite_sufficient: unique_required_high_plane_count == 1,
                pair_aware_selector_required: unique_required_high_plane_count > 1,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let required_variant_pages = variant_fe_pages
        .into_iter()
        .map(
            |((fd_source_page, required_high_plane), compatible_fe_source_pages)| {
                VariantRequirement {
                    fd_source_page,
                    required_high_plane_sha1: sha1_hex(&required_high_plane),
                    compatible_fe_source_pages: compatible_fe_source_pages.into_iter().collect(),
                }
            },
        )
        .collect::<Vec<_>>();
    let pair_aware_selector_required = fd_page_domains
        .iter()
        .any(|domain| domain.pair_aware_selector_required);
    let all_observed_pairs_exact_without_variants = required_variant_pages.is_empty();
    let observed_screen_count = observations
        .iter()
        .map(|observation| observation.screen_role)
        .collect::<BTreeSet<_>>()
        .len();

    Ok(TriggerPlaneReport {
        schema: 1,
        source_sha1: EXPECTED_SOURCE_SHA1,
        chr_4k_page_count: chr_page_count,
        trigger_timing: TriggerTiming {
            mmc4_fd_trigger_addresses: ["0x0FD8", "0x1FD8"],
            mapper165_fd_trigger_addresses: ["0x0FD0", "0x1FD0"],
            compared_bytes: "tile FD high plane at page offset 0x0FD8..0x0FDF",
            preservation_rule: "the selected FD page high plane must equal the paired FE page high plane",
        },
        observed_screen_count,
        unique_pair_count: pair_compatibility.len(),
        pair_compatibility,
        fd_page_domains,
        required_variant_page_count: required_variant_pages.len(),
        required_variant_pages,
        pair_aware_selector_required,
        all_observed_pairs_exact_without_variants,
        unresolved_boundaries: vec![
            "Observed pairs cover the documented title, early chapter, dialogue, status, and menu paths only.",
            "Battle, shop, item, save/load, chapter transition, defeat, and ending pairs remain unobserved.",
            "This report plans required CHR variants but does not allocate pages or patch runtime selectors.",
        ],
        release_eligible: false,
    })
}

fn fd_tile_high_plane(chr: &[u8], page: u8) -> Result<[u8; 8]> {
    let start = page as usize * CHR_PAGE_SIZE + FD_TILE_HIGH_PLANE_OFFSET;
    let end = start + 8;
    ensure!(end <= chr.len(), "CHR page {page:02X} is outside CHR");
    Ok(chr[start..end].try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chr_with_page_planes(planes: &[(u8, [u8; 8])]) -> Vec<u8> {
        let mut chr = vec![0; 32 * CHR_PAGE_SIZE];
        for (page, plane) in planes {
            let start = *page as usize * CHR_PAGE_SIZE + FD_TILE_HIGH_PLANE_OFFSET;
            chr[start..start + 8].copy_from_slice(plane);
        }
        chr
    }

    #[test]
    fn equal_fd_and_fe_high_planes_need_no_variant() {
        let plane = [0x20; 8];
        let chr = chr_with_page_planes(&[(0, plane), (1, plane)]);
        let report =
            analyze_observations(&chr, &[observed("screen", PatternWindow::Right, 0, 1)]).unwrap();

        assert!(report.all_observed_pairs_exact_without_variants);
        assert_eq!(report.required_variant_page_count, 0);
        assert!(!report.pair_aware_selector_required);
        assert_eq!(report.pair_compatibility[0].classification, "exact");
    }

    #[test]
    fn differing_high_planes_require_one_deduplicated_variant() {
        let chr = chr_with_page_planes(&[(0, [0x20; 8]), (1, [0; 8]), (2, [0; 8])]);
        let report = analyze_observations(
            &chr,
            &[
                observed("first", PatternWindow::Left, 0, 1),
                observed("second", PatternWindow::Right, 0, 2),
            ],
        )
        .unwrap();

        assert!(!report.all_observed_pairs_exact_without_variants);
        assert_eq!(report.required_variant_page_count, 1);
        assert_eq!(
            report.required_variant_pages[0].compatible_fe_source_pages,
            vec![1, 2]
        );
    }

    #[test]
    fn one_fd_page_with_conflicting_requirements_needs_pair_aware_selection() {
        let chr = chr_with_page_planes(&[(0, [0x20; 8]), (1, [0; 8]), (2, [0x20; 8])]);
        let report = analyze_observations(
            &chr,
            &[
                observed("mismatch", PatternWindow::Right, 0, 1),
                observed("natural", PatternWindow::Right, 0, 2),
            ],
        )
        .unwrap();

        assert!(report.pair_aware_selector_required);
        let domain = report
            .fd_page_domains
            .iter()
            .find(|domain| domain.fd_source_page == 0)
            .unwrap();
        assert_eq!(domain.unique_required_high_plane_count, 2);
        assert!(!domain.global_page_rewrite_sufficient);
        assert!(domain.pair_aware_selector_required);
    }

    #[test]
    fn duplicate_screen_observations_collapse_to_one_pair() {
        let chr = chr_with_page_planes(&[(0, [0; 8])]);
        let report = analyze_observations(
            &chr,
            &[
                observed("first", PatternWindow::Left, 0, 0),
                observed("second", PatternWindow::Left, 0, 0),
            ],
        )
        .unwrap();

        assert_eq!(report.observed_screen_count, 2);
        assert_eq!(report.unique_pair_count, 1);
        assert_eq!(
            report.pair_compatibility[0].screen_roles,
            vec!["first", "second"]
        );
    }

    #[test]
    fn runtime_variant_pairs_use_the_same_trigger_plane_rule() {
        let chr = chr_with_page_planes(&[
            (0, [0x20; 8]),
            (0x14, [0; 8]),
            (0x18, [0x20; 8]),
            (0x19, [0x20; 8]),
        ]);
        let pairs = observed_variant_pairs(&chr).unwrap();

        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].pattern_window, PatternWindow::Right);
        assert_eq!(pairs[0].fd_source_page, 0);
        assert_eq!(pairs[0].fe_source_page, 0x14);
        assert_eq!(pairs[0].required_high_plane, [0; 8]);
    }
}
