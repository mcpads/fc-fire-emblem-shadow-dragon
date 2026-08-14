use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

pub(super) struct CatalogNameDemand {
    pub(super) domain: &'static str,
    pub(super) source_index: usize,
    pub(super) additional_glyphs: BTreeSet<char>,
}

pub(super) struct CatalogNamePacking {
    pub(super) pages: Vec<BTreeSet<char>>,
    pub(super) identity_page_indices: BTreeMap<(&'static str, usize), usize>,
}

/// 큰 이름부터 넣고, 들어갈 수 있는 페이지 중 추가 합집합이 가장 작은 곳을 고른다.
/// 입력 순서와 해시 순회에 흔들리지 않도록 모든 동률은 도메인·원천 인덱스·페이지
/// 인덱스로 끊는다.
pub(super) fn pack_name_demands(
    demands: &[CatalogNameDemand],
    page_capacity: usize,
) -> Result<CatalogNamePacking> {
    ensure!(page_capacity > 0, "catalog name page has no free codes");
    ensure!(
        demands
            .iter()
            .all(|demand| demand.additional_glyphs.len() <= page_capacity),
        "one catalog name cannot fit the available per-page codes"
    );
    let mut order = (0..demands.len()).collect::<Vec<_>>();
    order.sort_by(|left, right| {
        demands[*right]
            .additional_glyphs
            .len()
            .cmp(&demands[*left].additional_glyphs.len())
            .then_with(|| demands[*left].domain.cmp(demands[*right].domain))
            .then_with(|| {
                demands[*left]
                    .source_index
                    .cmp(&demands[*right].source_index)
            })
    });

    let mut pages = Vec::<BTreeSet<char>>::new();
    let mut identity_page_indices = BTreeMap::new();
    for demand_index in order {
        let demand = &demands[demand_index];
        let selected = pages
            .iter()
            .enumerate()
            .filter_map(|(page_index, page)| {
                let merged_count = page.union(&demand.additional_glyphs).count();
                (merged_count <= page_capacity).then_some((merged_count, page_index))
            })
            .min();
        let page_index = if let Some((_, page_index)) = selected {
            pages[page_index].extend(&demand.additional_glyphs);
            page_index
        } else {
            pages.push(demand.additional_glyphs.clone());
            pages.len() - 1
        };
        ensure!(
            identity_page_indices
                .insert((demand.domain, demand.source_index), page_index)
                .is_none(),
            "catalog name identity is duplicated"
        );
    }
    ensure!(
        pages.iter().all(|page| page.len() <= page_capacity)
            && identity_page_indices.len() == demands.len(),
        "catalog name packing lost capacity or identity coverage"
    );
    Ok(CatalogNamePacking {
        pages,
        identity_page_indices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutually_exclusive_names_share_pages_without_losing_identity() {
        let demands = [
            demand("unit_names", 0, &['가', '나']),
            demand("unit_names", 1, &['나', '다']),
            demand("enemy_names", 0, &['라', '마']),
        ];

        let packing = pack_name_demands(&demands, 3).unwrap();

        assert_eq!(
            packing.pages,
            [
                BTreeSet::from(['라', '마']),
                BTreeSet::from(['가', '나', '다'])
            ]
        );
        assert_eq!(packing.identity_page_indices.len(), 3);
        assert_eq!(packing.identity_page_indices[&("unit_names", 0)], 1);
        assert_eq!(packing.identity_page_indices[&("unit_names", 1)], 1);
        assert_eq!(packing.identity_page_indices[&("enemy_names", 0)], 0);
    }

    #[test]
    fn one_name_larger_than_the_page_fails_closed() {
        let error = pack_name_demands(&[demand("unit_names", 0, &['가', '나'])], 1)
            .err()
            .expect("oversized name must fail");

        assert!(error.to_string().contains("one catalog name"));
    }

    fn demand(domain: &'static str, source_index: usize, glyphs: &[char]) -> CatalogNameDemand {
        CatalogNameDemand {
            domain,
            source_index,
            additional_glyphs: glyphs.iter().copied().collect(),
        }
    }
}
