use anyhow::{Result, ensure};

use crate::rom::{HEADER_SIZE, PRG_SIZE, Rom};

use super::{
    super::chapter_intro_residency::EncodedChapterTitle, FIXED_BANK_SIZE,
    technical_installation::IntegratedImage,
};

pub(super) fn install_encoded_chapter_titles(
    image: &mut IntegratedImage,
    candidate: &Rom,
    titles: &[EncodedChapterTitle],
) -> Result<()> {
    ensure!(
        titles.len() == 25,
        "integrated chapter-title write set must contain all twenty-five titles"
    );
    for title in titles {
        let end = title
            .file_offset
            .checked_add(title.encoded_storage.len())
            .ok_or_else(|| anyhow::anyhow!("{} storage range overflow", title.id))?;
        let expected = candidate
            .data()
            .get(title.file_offset..end)
            .ok_or_else(|| anyhow::anyhow!("{} storage is outside candidate", title.id))?;
        image.write_expected(
            format!("chapter title storage {}", title.id),
            title.file_offset,
            expected,
            &title.encoded_storage,
        )?;
        let active_mirror_offset = active_fixed_mirror_file_offset(candidate, title.file_offset)?;
        let active_mirror_end = active_mirror_offset
            .checked_add(title.encoded_storage.len())
            .ok_or_else(|| anyhow::anyhow!("{} active mirror range overflow", title.id))?;
        let active_mirror_expected = candidate
            .data()
            .get(active_mirror_offset..active_mirror_end)
            .ok_or_else(|| anyhow::anyhow!("{} active mirror is outside candidate", title.id))?;
        image.write_expected(
            format!("active fixed-bank chapter title mirror {}", title.id),
            active_mirror_offset,
            active_mirror_expected,
            &title.encoded_storage,
        )?;
    }
    Ok(())
}

pub(super) fn verify_installed_chapter_titles(
    installed: &[u8],
    candidate: &Rom,
    titles: &[EncodedChapterTitle],
) -> Result<()> {
    for title in titles {
        let end = title
            .file_offset
            .checked_add(title.encoded_storage.len())
            .ok_or_else(|| anyhow::anyhow!("{} installed range overflow", title.id))?;
        ensure!(
            installed.get(title.file_offset..end) == Some(title.encoded_storage.as_slice()),
            "installed {} does not match its resident codebook encoding",
            title.id
        );
        let active_mirror_offset = active_fixed_mirror_file_offset(candidate, title.file_offset)?;
        let active_mirror_end = active_mirror_offset
            .checked_add(title.encoded_storage.len())
            .ok_or_else(|| {
                anyhow::anyhow!("{} installed active mirror range overflow", title.id)
            })?;
        ensure!(
            installed.get(active_mirror_offset..active_mirror_end)
                == Some(title.encoded_storage.as_slice()),
            "installed active fixed-bank mirror {} does not match its resident codebook encoding",
            title.id
        );
    }
    Ok(())
}

pub(super) fn active_fixed_mirror_file_offset(
    candidate: &Rom,
    source_file_offset: usize,
) -> Result<usize> {
    let source_fixed_start = HEADER_SIZE + PRG_SIZE - FIXED_BANK_SIZE;
    let source_fixed_end = HEADER_SIZE + PRG_SIZE;
    ensure!(
        (source_fixed_start..source_fixed_end).contains(&source_file_offset),
        "chapter-title storage is outside the supported source fixed bank"
    );
    ensure!(
        candidate.prg().len() > PRG_SIZE,
        "integrated chapter-title installation requires an expanded active fixed bank"
    );
    let active_fixed_start = HEADER_SIZE
        + candidate
            .prg()
            .len()
            .checked_sub(FIXED_BANK_SIZE)
            .ok_or_else(|| anyhow::anyhow!("candidate PRG is smaller than one fixed bank"))?;
    Ok(active_fixed_start + source_file_offset - source_fixed_start)
}
