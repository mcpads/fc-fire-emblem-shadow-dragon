use std::ops::Range;

use anyhow::{Context, Result, ensure};
use serde::Serialize;

use crate::{rom::HEADER_SIZE, rom::Rom};

use super::{
    cold_request_presentation::ColdRequestPresentationPage,
    current_candidate::DialoguePagePoolCapacity,
    dialogue_bank_layout::{ACTIVE_FIXED_BANK, BATTLE_MATERIAL_BANK, PRG_BANK_SIZE},
};

const EXPECTED_MAPPER: u16 = 165;
const EXPECTED_PRG_SIZE: usize = 512 * 1024;
const PRG_PAGE_SIZE: usize = 8 * 1024;
const MAIN_DIALOGUE_RUNTIME_FIRST_PAGE: u8 = 0x2C;
const LAST_SWITCHABLE_MATERIAL_PAGE: u8 = 0x3D;
const MINIMUM_REPORTED_FIXED_CAVE_SIZE: usize = 32;

#[derive(Serialize)]
pub(super) struct InstallationLayoutPlan {
    current_candidate_mapper: u16,
    current_candidate_prg_size: usize,
    existing_battle_material_bank: u8,
    main_dialogue_runtime_material: MaterialPageReservation,
    remaining_cross_domain_material_pool: MaterialPageReservation,
    fixed_code_reservations: Vec<FixedCodeReservation>,
    remaining_fixed_code_caves: Vec<FixedCodeCave>,
    cold_request_presentation_chr_page: ChrPageReservation,
    remaining_reclaimable_chr_page_first: u8,
    remaining_reclaimable_chr_page_count: usize,
    all_reserved_material_bytes_are_exact_ff: bool,
    reservations_are_disjoint: bool,
    cross_domain_material_capacity_bound: bool,
}

#[derive(Serialize)]
struct MaterialPageReservation {
    role: &'static str,
    first_mmc3_page: u8,
    page_count: usize,
    byte_count: usize,
    capacity_byte_count: usize,
}

#[derive(Serialize)]
struct FixedCodeReservation {
    role: &'static str,
    cpu_start_hex: String,
    cpu_end_exclusive_hex: String,
    byte_count: usize,
    source_is_exact_ff: bool,
}

#[derive(Serialize)]
struct FixedCodeCave {
    cpu_start_hex: String,
    cpu_end_exclusive_hex: String,
    byte_count: usize,
}

#[derive(Serialize)]
struct ChrPageReservation {
    role: &'static str,
    physical_page: u8,
    mapper_register: u8,
    byte_count: usize,
    blanked_code_count: usize,
    sha1: String,
}

pub(super) fn plan_installation_layout(
    candidate: &Rom,
    page_pool: &DialoguePagePoolCapacity,
    main_dialogue_runtime_material_byte_count: usize,
    cold_request_presentation: &ColdRequestPresentationPage,
) -> Result<InstallationLayoutPlan> {
    ensure!(
        candidate.mapper() == EXPECTED_MAPPER && candidate.prg().len() == EXPECTED_PRG_SIZE,
        "integrated installation layout requires the current 512 KiB mapper 165 candidate"
    );
    ensure!(
        BATTLE_MATERIAL_BANK == 0x10 && ACTIVE_FIXED_BANK == 0x1F,
        "expanded PRG ownership changed before integrated layout planning"
    );

    let main_dialogue_page_count =
        main_dialogue_runtime_material_byte_count.div_ceil(PRG_PAGE_SIZE);
    ensure!(
        main_dialogue_page_count > 0,
        "main-dialogue runtime material reservation is empty"
    );
    let first_cross_domain_page = MAIN_DIALOGUE_RUNTIME_FIRST_PAGE
        .checked_add(
            u8::try_from(main_dialogue_page_count)
                .context("main-dialogue runtime page count does not fit u8")?,
        )
        .context("main-dialogue runtime page range overflow")?;
    ensure!(
        first_cross_domain_page <= LAST_SWITCHABLE_MATERIAL_PAGE,
        "main-dialogue runtime material consumes the entire expanded PRG pool"
    );
    let cross_domain_page_count =
        usize::from(LAST_SWITCHABLE_MATERIAL_PAGE + 1 - first_cross_domain_page);

    let main_dialogue_range =
        mmc3_page_file_range(MAIN_DIALOGUE_RUNTIME_FIRST_PAGE, main_dialogue_page_count)?;
    let cross_domain_range =
        mmc3_page_file_range(first_cross_domain_page, cross_domain_page_count)?;
    ensure!(
        main_dialogue_range.end <= cross_domain_range.start,
        "main-dialogue and cross-domain material reservations overlap"
    );
    ensure!(
        candidate
            .data()
            .get(main_dialogue_range.clone())
            .context("main-dialogue material reservation is outside candidate")?
            .iter()
            .all(|byte| *byte == 0xFF)
            && candidate
                .data()
                .get(cross_domain_range)
                .context("cross-domain material pool is outside candidate")?
                .iter()
                .all(|byte| *byte == 0xFF),
        "integrated material reservation no longer points to exact FF bytes"
    );

    let fixed_bank = candidate
        .prg()
        .get(usize::from(ACTIVE_FIXED_BANK) * PRG_BANK_SIZE..)
        .context("active fixed bank is outside candidate PRG")?;
    ensure!(
        fixed_bank.len() == PRG_BANK_SIZE,
        "active fixed bank length changed"
    );
    // 전이 reader 예약은 이중 진입과 함께 폐기했다. 고정 뱅크의 빈 구간은 전부
    // 후속 코드 후보로 돌려준다. 의사결정 59번을 따른다.
    let fixed_code_reservations: Vec<FixedCodeReservation> = Vec::new();
    let remaining_fixed_code_caves = exact_ff_ranges(fixed_bank)
        .into_iter()
        .filter(|range| range.len() >= MINIMUM_REPORTED_FIXED_CAVE_SIZE)
        .map(|range| FixedCodeCave {
            cpu_start_hex: format!("{:04X}", range.start),
            cpu_end_exclusive_hex: format!("{:04X}", range.end),
            byte_count: range.len(),
        })
        .collect::<Vec<_>>();
    ensure!(
        !remaining_fixed_code_caves.is_empty(),
        "integrated layout found no remaining fixed-bank code cave"
    );
    ensure!(
        page_pool.available_page_count > 0
            && cold_request_presentation.physical_page == page_pool.first_installable_physical_page,
        "cold-request presentation does not reserve the first reclaimable CHR page"
    );
    let remaining_reclaimable_chr_page_first = cold_request_presentation
        .physical_page
        .checked_add(1)
        .context("remaining reclaimable CHR page range overflow")?;
    let remaining_reclaimable_chr_page_count = page_pool
        .available_page_count
        .checked_sub(1)
        .context("cold-request presentation exhausted the reclaimable CHR page pool")?;

    Ok(InstallationLayoutPlan {
        current_candidate_mapper: candidate.mapper(),
        current_candidate_prg_size: candidate.prg().len(),
        existing_battle_material_bank: BATTLE_MATERIAL_BANK,
        main_dialogue_runtime_material: MaterialPageReservation {
            role: "main_dialogue_runtime_material",
            first_mmc3_page: MAIN_DIALOGUE_RUNTIME_FIRST_PAGE,
            page_count: main_dialogue_page_count,
            byte_count: main_dialogue_runtime_material_byte_count,
            capacity_byte_count: main_dialogue_page_count * PRG_PAGE_SIZE,
        },
        remaining_cross_domain_material_pool: MaterialPageReservation {
            role: "remaining_cross_domain_material_pool",
            first_mmc3_page: first_cross_domain_page,
            page_count: cross_domain_page_count,
            byte_count: 0,
            capacity_byte_count: cross_domain_page_count * PRG_PAGE_SIZE,
        },
        fixed_code_reservations,
        remaining_fixed_code_caves,
        cold_request_presentation_chr_page: ChrPageReservation {
            role: "cold_request_dialogue_presentation",
            physical_page: cold_request_presentation.physical_page,
            mapper_register: cold_request_presentation.mapper_register,
            byte_count: cold_request_presentation.bytes.len(),
            blanked_code_count: cold_request_presentation.blanked_code_count,
            sha1: cold_request_presentation.sha1.clone(),
        },
        remaining_reclaimable_chr_page_first,
        remaining_reclaimable_chr_page_count,
        all_reserved_material_bytes_are_exact_ff: true,
        reservations_are_disjoint: true,
        cross_domain_material_capacity_bound: false,
    })
}

pub(super) fn main_dialogue_runtime_material_file_offset() -> Result<usize> {
    Ok(mmc3_page_file_range(MAIN_DIALOGUE_RUNTIME_FIRST_PAGE, 1)?.start)
}

fn mmc3_page_file_range(first_page: u8, page_count: usize) -> Result<Range<usize>> {
    let start = HEADER_SIZE
        .checked_add(usize::from(first_page) * PRG_PAGE_SIZE)
        .context("MMC3 material page start overflow")?;
    let end = start
        .checked_add(page_count * PRG_PAGE_SIZE)
        .context("MMC3 material page end overflow")?;
    Ok(start..end)
}

fn exact_ff_ranges(fixed_bank: &[u8]) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = None;
    for (offset, byte) in fixed_bank.iter().copied().enumerate() {
        if byte == 0xFF {
            start.get_or_insert(offset);
        } else if let Some(range_start) = start.take() {
            ranges.push(cpu_range(range_start, offset));
        }
    }
    if let Some(range_start) = start {
        ranges.push(cpu_range(range_start, fixed_bank.len()));
    }
    ranges
}

fn cpu_range(start: usize, end: usize) -> Range<usize> {
    0xC000 + start..0xC000 + end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_kibibytes_reserve_three_mmc3_pages() {
        assert_eq!(20_246usize.div_ceil(PRG_PAGE_SIZE), 3);
        assert_eq!(MAIN_DIALOGUE_RUNTIME_FIRST_PAGE + 3, 0x2F);
        assert_eq!(usize::from(LAST_SWITCHABLE_MATERIAL_PAGE + 1 - 0x2F), 15);
    }

}
