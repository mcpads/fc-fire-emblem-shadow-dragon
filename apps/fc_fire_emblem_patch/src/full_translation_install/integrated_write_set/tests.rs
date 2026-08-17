use super::*;
use crate::dialogue_assets::{EncodedMainDialogueRegion, MainDialoguePointerWrite};

fn synthetic_rom() -> Rom {
    let mut bytes = vec![0; crate::rom::HEADER_SIZE + 16 * 1024];
    bytes[..4].copy_from_slice(b"NES\x1A");
    bytes[4] = 1;
    Rom::parse(bytes).unwrap()
}

#[test]
fn installs_encoded_storage_and_pointers_in_one_tracked_image() {
    let candidate = synthetic_rom();
    let encoded = EncodedMainDialogueBundle {
        regions: vec![EncodedMainDialogueRegion {
            file_offset: 0x20,
            source_storage: vec![0, 0, 0],
            encoded_storage: vec![0x40, 0x41, 0xEF],
            used_storage_byte_count: 3,
        }],
        pointer_writes: vec![MainDialoguePointerWrite {
            record_id: "record".to_owned(),
            file_offset: 0x30,
            source_pointer: 0x8000,
            planned_pointer: 0x8123,
        }],
    };
    let mut image = IntegratedImage::new(candidate.data().to_vec(), None);

    install_encoded_dialogue(&mut image, &candidate, &encoded).unwrap();

    assert_eq!(image.writes().len(), 2);
    let output = image.into_data();
    verify_installed_dialogue(&output, &encoded).unwrap();
    assert_eq!(&output[0x20..0x23], [0x40, 0x41, 0xEF]);
    assert_eq!(&output[0x30..0x32], 0x8123_u16.to_le_bytes());
}

#[test]
fn installs_all_chapter_titles_and_verifies_their_final_bytes() {
    let candidate = synthetic_expanded_rom();
    let source_fixed_start = crate::rom::HEADER_SIZE + crate::rom::PRG_SIZE - FIXED_BANK_SIZE;
    let titles = (0..25)
        .map(|index| EncodedChapterTitle {
            id: format!("chapter-title:{:03}", index + 1),
            file_offset: source_fixed_start + 0x100 + index * 2,
            encoded_storage: vec![index as u8 + 1, 0xED],
        })
        .collect::<Vec<_>>();
    let mut image = IntegratedImage::new(candidate.data().to_vec(), None);

    install_encoded_chapter_titles(&mut image, &candidate, &titles).unwrap();

    assert_eq!(image.writes().len(), 50);
    let output = image.into_data();
    verify_installed_chapter_titles(&output, &candidate, &titles).unwrap();
    assert_eq!(
        &output[source_fixed_start + 0x100..source_fixed_start + 0x104],
        [1, 0xED, 2, 0xED]
    );
    let active_fixed_start = crate::rom::HEADER_SIZE + 512 * 1024 - FIXED_BANK_SIZE;
    assert_eq!(
        &output[active_fixed_start + 0x100..active_fixed_start + 0x104],
        [1, 0xED, 2, 0xED]
    );
}

fn synthetic_expanded_rom() -> Rom {
    let mut bytes = vec![0; crate::rom::HEADER_SIZE + 512 * 1024];
    bytes[..4].copy_from_slice(b"NES\x1A");
    bytes[4] = 32;
    Rom::parse(bytes).unwrap()
}
