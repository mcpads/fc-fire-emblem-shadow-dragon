//! 누적 무기점 전용 포인터를 최종 공용 텍스트 소비자로 환원한다.
//!
//! 누적 후보는 무기점만을 위한 글꼴과 문자열을 썼으므로 선택지와 선택 품목을
//! 고정 뱅크 선택기로 우회했다. 최종 설치에서는 선택지가 모든 판매 시설에 같은
//! 코드로 거주하고 품목 생산자도 정규 코드 저장소를 직접 채운다. 따라서 예전
//! 선택기로 다시 들어가면 두 코드북 소유자가 동시에 살아난다. 이 계획은 그 두
//! 호출 자리를 원본의 표 조회로 되돌리고 최종 런타임 훅이 각각의 실제 생산 경로를
//! 소유하는지 한 단위로 묶는다.

use std::collections::BTreeSet;

use anyhow::{Result, ensure};
use serde::Serialize;

use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    mapper165::{
        CHOICE_POINTER_LOAD_ADDRESS, CHOICE_POINTER_LOAD_PRG_BANK, FontPageFallbackNodeRole,
        SELECTED_ITEM_POINTER_LOAD_ADDRESS, SELECTED_ITEM_POINTER_LOAD_PRG_BANK,
        build_choice_pointer_load_call, build_original_choice_pointer_load,
        build_original_selected_item_pointer_load, build_selected_item_pointer_load_call,
    },
    rom::Rom,
};

use super::{
    choice_residency::ChoiceResidencyPlan,
    runtime_code::{DialogueRuntimeCodePlan, DialogueRuntimeHookRole},
    screen_font_residency::FontPageSelectorForwarderPlan,
    shop_item_residency::ShopItemResidencyPlan,
};

const CHOICE_DOMAINS: &[&str] = &["choice_labels"];
const ITEM_DOMAINS: &[&str] = &["item_names"];

pub(super) struct ShopTextConsumerInputs<'a> {
    pub(super) candidate: &'a Rom,
    pub(super) runtime_code: &'a DialogueRuntimeCodePlan,
    pub(super) choice_residency: &'a ChoiceResidencyPlan,
    pub(super) shop_item_residency: &'a ShopItemResidencyPlan,
    pub(super) selector_forwarders: &'a FontPageSelectorForwarderPlan,
}

#[derive(Serialize)]
pub(super) struct ShopTextConsumerPlan {
    schema: u8,
    strategy: &'static str,
    restored_pointer_load_count: usize,
    projected_choice_screen_role_count: usize,
    projected_item_screen_role_count: usize,
    shop_item_list_appender_bound: bool,
    dynamic_item_producer_bound: bool,
    legacy_choice_selector_unreachable: bool,
    legacy_selected_item_selector_unreachable: bool,
    legacy_weapon_font_selector_unreachable: bool,
    one_final_owner_per_shop_text_route: bool,
    #[serde(skip)]
    writes: Vec<ShopTextConsumerExpectedWrite>,
}

impl ShopTextConsumerPlan {
    pub(super) fn writes(&self) -> &[ShopTextConsumerExpectedWrite] {
        &self.writes
    }

    pub(super) fn write_count(&self) -> usize {
        self.writes.len()
    }

    pub(super) fn write_count_for_domain(&self, domain: &str) -> usize {
        self.writes
            .iter()
            .filter(|write| write.domains.contains(&domain))
            .count()
    }
}

pub(super) struct ShopTextConsumerExpectedWrite {
    pub(super) domains: &'static [&'static str],
    pub(super) role: &'static str,
    pub(super) prg_bank: u8,
    pub(super) cpu_address: u16,
    pub(super) file_offset: usize,
    pub(super) expected: Vec<u8>,
    pub(super) replacement: Vec<u8>,
}

pub(super) fn plan_shop_text_consumers(
    inputs: ShopTextConsumerInputs<'_>,
) -> Result<ShopTextConsumerPlan> {
    let hook_roles = inputs
        .runtime_code
        .hook_roles()
        .into_iter()
        .collect::<BTreeSet<_>>();
    ensure!(
        hook_roles.contains(&DialogueRuntimeHookRole::ShopItemListAppender)
            && hook_roles.contains(&DialogueRuntimeHookRole::DynamicItemSlotProducer),
        "final shop text consumers lost the item-list or selected-item runtime owner"
    );
    let choice_screen_roles = inputs.choice_residency.projected_shop_screen_roles();
    let item_screen_roles = inputs.shop_item_residency.projected_shop_screen_roles()?;
    ensure!(
        !choice_screen_roles.is_empty() && !item_screen_roles.is_empty(),
        "final shop text consumer projection has no declared screens"
    );
    ensure!(
        inputs
            .selector_forwarders
            .replaces_source_role(FontPageFallbackNodeRole::WeaponShopDialogue),
        "final shop text route still retains the cumulative weapon-only font selector"
    );

    let writes = vec![
        bind_pointer_restoration(
            inputs.candidate,
            CHOICE_DOMAINS,
            "restore the shared choice pointer load after removing the weapon-only selector",
            CHOICE_POINTER_LOAD_PRG_BANK,
            CHOICE_POINTER_LOAD_ADDRESS,
            build_choice_pointer_load_call()?,
            build_original_choice_pointer_load()?,
        )?,
        bind_pointer_restoration(
            inputs.candidate,
            ITEM_DOMAINS,
            "restore the selected-item pointer load after the canonical producer supersedes it",
            SELECTED_ITEM_POINTER_LOAD_PRG_BANK,
            SELECTED_ITEM_POINTER_LOAD_ADDRESS,
            build_selected_item_pointer_load_call()?,
            build_original_selected_item_pointer_load()?,
        )?,
    ];
    ensure!(
        writes.iter().all(|write| write.expected.len() == 10
            && write.expected.len() == write.replacement.len()
            && write.expected != write.replacement),
        "shop text pointer restoration does not own two changed ten-byte source spans"
    );

    Ok(ShopTextConsumerPlan {
        schema: 1,
        strategy: "remove the two cumulative weapon-only pointer decisions after binding the final shared choice residency, canonical selected-item producer, and global shop item-list appender; restore the typed original table loads so every live shop text route has one final owner",
        restored_pointer_load_count: writes.len(),
        projected_choice_screen_role_count: choice_screen_roles.len(),
        projected_item_screen_role_count: item_screen_roles.len(),
        shop_item_list_appender_bound: true,
        dynamic_item_producer_bound: true,
        legacy_choice_selector_unreachable: true,
        legacy_selected_item_selector_unreachable: true,
        legacy_weapon_font_selector_unreachable: true,
        one_final_owner_per_shop_text_route: true,
        writes,
    })
}

fn bind_pointer_restoration(
    candidate: &Rom,
    domains: &'static [&'static str],
    role: &'static str,
    prg_bank: u8,
    cpu_address: u16,
    expected: Vec<u8>,
    replacement: Vec<u8>,
) -> Result<ShopTextConsumerExpectedWrite> {
    let file_offset = switchable_cpu_to_file_offset(prg_bank, cpu_address)?;
    ensure!(
        candidate
            .data()
            .get(file_offset..file_offset + expected.len())
            == Some(expected.as_slice()),
        "exact candidate changed before {role}"
    );
    Ok(ShopTextConsumerExpectedWrite {
        domains,
        role,
        prg_bank,
        cpu_address,
        file_offset,
        expected,
        replacement,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_restorations_return_to_the_source_tables() {
        assert_eq!(
            build_original_choice_pointer_load().unwrap(),
            [0xB9, 0xC2, 0x8F, 0x85, 0x00, 0xB9, 0xC3, 0x8F, 0x85, 0x01]
        );
        assert_eq!(
            build_original_selected_item_pointer_load().unwrap(),
            [0xB9, 0xD5, 0xDA, 0x85, 0x00, 0xB9, 0xD6, 0xDA, 0x85, 0x01]
        );
    }
}
