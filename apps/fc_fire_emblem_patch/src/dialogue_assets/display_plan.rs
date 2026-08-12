//! 정규 레코드에서 화면 표시 순서와 페이지 작업집합을 만든다.
//!
//! 전에는 이중 진입 작업공간 안에 있었고 직접 진입과 전이 진입을 따로 펼쳐
//! 레코드 하나가 표시 경로 둘을 가질 수 있었다. 두 모드의 차이는 레코드 프리픽스
//! 파서 결함이 만든 것이어서 폐기했다. 이제 표시 단위는 정규 레코드 그 자체다.
//! 의사결정 59번을 따른다.

use std::{collections::BTreeSet, ops::Range};

use anyhow::Result;

use super::{MainDialogueBundlePlan, MainDialoguePageWorkset};

pub(crate) struct MainDialogueDisplayPlan {
    pub(crate) canonical_record_count: usize,
    pub(crate) page_worksets: Vec<MainDialoguePageWorkset>,
    /// 표시 순서대로 늘어놓은 정규 레코드 ID다. 런타임 식별표가 쓰는 색인이 이 순서다.
    pub(crate) record_ids: Vec<String>,
    /// 같은 순서로 늘어놓은 레코드별 가시 페이지 구간이다.
    pub(crate) visible_page_ranges: Vec<Vec<Range<usize>>>,
}

impl MainDialogueDisplayPlan {
    pub(crate) fn from_canonical_bundle(dialogue: &MainDialogueBundlePlan) -> Result<Self> {
        Ok(Self {
            canonical_record_count: dialogue.record_ids.len(),
            page_worksets: dialogue.page_worksets.clone(),
            record_ids: dialogue.record_ids.clone(),
            visible_page_ranges: dialogue.canonical_visible_page_ranges()?,
        })
    }

    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.page_worksets
            .iter()
            .flat_map(|workset| workset.target_glyphs.iter().copied())
            .collect()
    }
}
