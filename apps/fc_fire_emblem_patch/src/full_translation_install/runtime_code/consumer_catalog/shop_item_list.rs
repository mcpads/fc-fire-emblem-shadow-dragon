use anyhow::{Context, Result, ensure};

use super::{KIND_SHOP_ITEM_LIST, SEGMENT_SEPARATOR};
use crate::{
    dialogue_inventory::switchable_cpu_to_file_offset,
    full_translation_install::runtime_code::{
        DialogueRuntimeHook, DialogueRuntimeHookRole, DialogueRuntimeHookSite,
    },
    mapper165::{
        ITEM_LIST_POINTER_LOAD_ADDRESS, ITEM_LIST_POINTER_LOAD_BYTES,
        ITEM_LIST_POINTER_LOAD_PRG_BANK, build_item_list_pointer_load_call,
    },
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    typed_source::decode_rp2a03_sequence,
};

const ORIGINAL_COPY_AND_RETURN_ADDRESS: u16 = 0x8E7E;
const ORIGINAL_RETURN_ADDRESS: u16 = 0x8E87;
const ORIGINAL_COPY_AND_RETURN_BYTES: [u8; 10] =
    [0x20, 0xFA, 0x8E, 0xA9, 0xED, 0x9D, 0x51, 0x04, 0xE8, 0x60];

pub(super) fn bind_site(source: &Rom, candidate: &Rom) -> Result<()> {
    let pointer_offset = switchable_cpu_to_file_offset(
        ITEM_LIST_POINTER_LOAD_PRG_BANK,
        ITEM_LIST_POINTER_LOAD_ADDRESS,
    )?;
    ensure!(
        source
            .data()
            .get(pointer_offset..pointer_offset + ITEM_LIST_POINTER_LOAD_BYTES.len())
            == Some(ITEM_LIST_POINTER_LOAD_BYTES.as_slice()),
        "source shop item-list pointer consumer changed at 0B:8E74"
    );
    let previous_projection = build_item_list_pointer_load_call()?;
    ensure!(
        candidate
            .data()
            .get(pointer_offset..pointer_offset + previous_projection.len())
            == Some(previous_projection.as_slice()),
        "candidate shop item-list pointer consumer is no longer the superseded weapon-only projection"
    );

    let continuation_offset = switchable_cpu_to_file_offset(
        ITEM_LIST_POINTER_LOAD_PRG_BANK,
        ORIGINAL_COPY_AND_RETURN_ADDRESS,
    )?;
    for (role, rom) in [("source", source), ("candidate", candidate)] {
        let bytes = rom
            .data()
            .get(continuation_offset..continuation_offset + ORIGINAL_COPY_AND_RETURN_BYTES.len())
            .with_context(|| format!("{role} shop item-list continuation is outside the ROM"))?;
        ensure!(
            bytes == ORIGINAL_COPY_AND_RETURN_BYTES,
            "{role} shop item-list copy-and-return continuation changed"
        );
        decode_rp2a03_sequence(
            bytes,
            ORIGINAL_COPY_AND_RETURN_ADDRESS,
            "shop item-list copy-and-return continuation",
        )?;
    }
    Ok(())
}

pub(super) fn build_hook(bridge: u16) -> Result<DialogueRuntimeHook> {
    let bytes = assemble_at(
        ITEM_LIST_POINTER_LOAD_ADDRESS,
        &[
            Instruction::LdaImmediate(KIND_SHOP_ITEM_LIST),
            Instruction::JsrAbsolute(bridge),
            Instruction::LdaImmediate(SEGMENT_SEPARATOR),
            Instruction::JmpAbsolute(ORIGINAL_RETURN_ADDRESS),
        ],
    )?;
    ensure!(
        bytes.len() == ITEM_LIST_POINTER_LOAD_BYTES.len(),
        "shop item-list appender hook no longer exactly replaces the superseded pointer selector"
    );
    Ok(DialogueRuntimeHook {
        role: DialogueRuntimeHookRole::ShopItemListAppender,
        write_role: "shop item-list canonical appender hook",
        site: DialogueRuntimeHookSite::Switchable {
            bank: ITEM_LIST_POINTER_LOAD_PRG_BANK,
            address: ITEM_LIST_POINTER_LOAD_ADDRESS,
        },
        bytes,
    })
}

pub(super) fn verify_hook(hooks: &[DialogueRuntimeHook], bridge: u16) -> Result<()> {
    let matching = hooks
        .iter()
        .filter(|hook| hook.role == DialogueRuntimeHookRole::ShopItemListAppender)
        .collect::<Vec<_>>();
    ensure!(
        matching.len() == 1,
        "shop item-list route has {} runtime hooks",
        matching.len()
    );
    let expected = build_hook(bridge)?;
    let hook = matching[0];
    ensure!(
        matches!(
            hook.site,
            DialogueRuntimeHookSite::Switchable {
                bank: ITEM_LIST_POINTER_LOAD_PRG_BANK,
                address: ITEM_LIST_POINTER_LOAD_ADDRESS,
            }
        ) && hook.bytes == expected.bytes,
        "shop item-list runtime hook no longer enters the canonical appender and returns from the source routine"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_replaces_only_the_old_pointer_selector_and_returns_from_the_source_routine() {
        let hook = build_hook(0xFAF3).unwrap();

        assert_eq!(hook.bytes.len(), ITEM_LIST_POINTER_LOAD_BYTES.len());
        assert_eq!(
            hook.bytes,
            [
                0xA9,
                KIND_SHOP_ITEM_LIST,
                0x20,
                0xF3,
                0xFA,
                0xA9,
                SEGMENT_SEPARATOR,
                0x4C,
                ORIGINAL_RETURN_ADDRESS as u8,
                (ORIGINAL_RETURN_ADDRESS >> 8) as u8,
            ]
        );
    }
}
