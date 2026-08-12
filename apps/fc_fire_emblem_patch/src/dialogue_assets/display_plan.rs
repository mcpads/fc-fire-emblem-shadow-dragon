//! 정규 레코드에서 화면 표시 경로와 페이지 작업집합을 만든다.
//!
//! 전에는 이중 진입 작업공간 안에 있었고 직접 진입과 전이 진입을 따로 펼쳤다.
//! 두 모드의 차이는 레코드 프리픽스 파서 결함이 만든 것이어서 폐기했다.
//! 이제 표시 경로는 정규 레코드 하나당 하나다. 의사결정 59번을 따른다.

use std::collections::BTreeSet;

use anyhow::Result;

use super::{MainDialogueBundlePlan, MainDialogueDisplayPath, MainDialoguePageWorkset};

pub(crate) struct MainDialogueDisplayPlan {
    pub(crate) canonical_record_count: usize,
    pub(crate) display_path_count: usize,
    pub(crate) ordinary_record_count: usize,
    pub(crate) page_worksets: Vec<MainDialoguePageWorkset>,
    pub(crate) display_paths: Vec<MainDialogueDisplayPath>,
}

impl MainDialogueDisplayPlan {
    pub(crate) fn from_canonical_bundle(dialogue: &MainDialogueBundlePlan) -> Result<Self> {
        Ok(Self {
            canonical_record_count: dialogue.record_ids.len(),
            display_path_count: dialogue.record_ids.len(),
            ordinary_record_count: dialogue.record_ids.len(),
            page_worksets: dialogue.page_worksets.clone(),
            display_paths: dialogue.canonical_display_paths()?,
        })
    }

    pub(crate) fn unique_glyphs(&self) -> BTreeSet<char> {
        self.page_worksets
            .iter()
            .flat_map(|workset| workset.target_glyphs.iter().copied())
            .collect()
    }
}
