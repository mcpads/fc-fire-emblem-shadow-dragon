use anyhow::{Result, ensure};

use crate::{dialogue_assets::EncodedMainDialogueBundle, rom::Rom};

use super::technical_installation::IntegratedImage;

/// 최종 바이트를 다시 읽어 현재 인코딩 결과가 실제 설치됐는지 확인한다.
///
/// 계획 개수나 `TrackedImage` 등록만 확인하면 런타임 재료는 새 코드북인데 본문은 이전
/// 단계 코드북인 산출물도 만들 수 있다. 최종 산출물의 소유 구간과 포인터 바이트가
/// 현재 번들의 결과와 하나라도 다르면 빌드를 실패시킨다.
pub(super) fn verify_installed_dialogue(
    installed: &[u8],
    encoded: &EncodedMainDialogueBundle,
) -> Result<()> {
    for (region_index, region) in encoded.regions.iter().enumerate() {
        let end = region
            .file_offset
            .checked_add(region.encoded_storage.len())
            .ok_or_else(|| anyhow::anyhow!("installed dialogue region range overflow"))?;
        ensure!(
            installed.get(region.file_offset..end) == Some(region.encoded_storage.as_slice()),
            "installed dialogue region {region_index} does not match the current encoding"
        );
    }
    for pointer in &encoded.pointer_writes {
        ensure!(
            installed.get(pointer.file_offset..pointer.file_offset + 2)
                == Some(pointer.planned_pointer.to_le_bytes().as_slice()),
            "installed dialogue pointer {} does not match the current encoding",
            pointer.record_id
        );
    }
    Ok(())
}

/// 현재 단계 후보에 정규 대사 저장소와 포인터를 함께 설치한다.
///
/// 후보 전체의 SHA-1은 호출자가 이미 빌드 보고서와 결속했다. 여기서는 그 정확한
/// 후보 바이트를 Expected Write의 선행조건으로 삼고, 현재 코드북으로 다시 만든
/// 저장소와 포인터를 한 이미지에 등록한다. 런타임 재료만 새로 쓰고 이전 단계의
/// 코드북 바이트를 남겨 두는 산출물은 이 경계를 통과할 수 없다.
pub(super) fn install_encoded_dialogue(
    image: &mut IntegratedImage,
    candidate: &Rom,
    encoded: &EncodedMainDialogueBundle,
) -> Result<()> {
    for (region_index, region) in encoded.regions.iter().enumerate() {
        ensure!(
            region.encoded_storage.len() == region.source_storage.len(),
            "encoded dialogue region {region_index} changed its owned extent"
        );
        let end = region
            .file_offset
            .checked_add(region.encoded_storage.len())
            .ok_or_else(|| anyhow::anyhow!("encoded dialogue region range overflow"))?;
        let expected = candidate
            .data()
            .get(region.file_offset..end)
            .ok_or_else(|| anyhow::anyhow!("encoded dialogue region is outside candidate"))?;
        image.write_expected(
            format!("main dialogue storage region {region_index}"),
            region.file_offset,
            expected,
            &region.encoded_storage,
        )?;
    }

    for pointer in &encoded.pointer_writes {
        let expected = candidate
            .data()
            .get(pointer.file_offset..pointer.file_offset + 2)
            .ok_or_else(|| anyhow::anyhow!("main dialogue pointer is outside candidate"))?;
        image.write_expected(
            format!("main dialogue pointer {}", pointer.record_id),
            pointer.file_offset,
            expected,
            &pointer.planned_pointer.to_le_bytes(),
        )?;
    }
    Ok(())
}
