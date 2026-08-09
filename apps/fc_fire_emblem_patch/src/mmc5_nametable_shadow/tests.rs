use super::*;

#[test]
fn direct_ppu_store_sites_cover_twenty_address_bytes_and_seven_data_consumers() {
    assert_eq!(
        PPU_STORE_SITES
            .iter()
            .filter(|site| site.kind != PpuStoreKind::Data)
            .count(),
        20
    );
    assert_eq!(
        PPU_STORE_SITES
            .iter()
            .filter(|site| site.kind == PpuStoreKind::Data)
            .count(),
        7
    );
    let mut offsets = PPU_STORE_SITES
        .iter()
        .map(|site| site.file_offset().unwrap())
        .collect::<Vec<_>>();
    offsets.sort_unstable();
    offsets.dedup();
    assert_eq!(offsets.len(), PPU_STORE_SITES.len());
}

#[test]
fn fixed_hooks_preserve_the_original_store_before_recording_it() {
    for (register, operation) in [
        (PPU_ADDRESS_REGISTER, OPERATION_ADDRESS_HIGH),
        (PPU_ADDRESS_REGISTER, OPERATION_ADDRESS_LOW),
        (PPU_DATA_REGISTER, OPERATION_DATA),
    ] {
        let hook = ppu_store_hook(register, operation).unwrap();
        assert_eq!(&hook[..3], &[0x8D, register as u8, (register >> 8) as u8]);
        assert_eq!(hook.len(), 13);
    }
}

#[test]
fn runtime_payload_routines_fit_the_two_page_install_image() {
    let payload = runtime_payload().unwrap();
    let initializer = runtime_initialize().unwrap();
    assert_eq!(payload.len(), RUNTIME_PAYLOAD_LEN);
    assert!(initializer.len() < 0x100);
    assert_eq!(
        &payload[usize::from(RUNTIME_INITIALIZE_ADDRESS - RUNTIME_DISPATCH_ADDRESS)
            ..usize::from(RUNTIME_INITIALIZE_ADDRESS - RUNTIME_DISPATCH_ADDRESS)
                + initializer.len()],
        initializer
    );
}

#[test]
fn physical_shadow_uses_two_kibibytes_outside_runtime_code_and_state() {
    assert_eq!(PHYSICAL_NAMETABLE_END - PHYSICAL_NAMETABLE_START, 0x0800);
    assert!(RUNTIME_DISPATCH_ADDRESS + RUNTIME_PAYLOAD_LEN as u16 <= RUNTIME_STATE_START);
    assert!(RUNTIME_STATE_START + u16::from(RUNTIME_STATE_LEN) <= PHYSICAL_NAMETABLE_START);
}
