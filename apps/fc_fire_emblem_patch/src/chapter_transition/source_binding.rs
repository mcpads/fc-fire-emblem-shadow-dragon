use super::*;

fn validated_chapter_intro_contexts(
    rom: &Rom,
) -> Result<Vec<crate::dialogue_inventory::ChapterIntroContextBinding>> {
    let mut contexts = inspect_chapter_intro_contexts(rom.data())?;
    contexts.sort_by_key(|context| context.chapter_index);
    ensure!(
        contexts.len() == CHAPTER_TITLE_COUNT,
        "expected {CHAPTER_TITLE_COUNT} chapter-intro E5 contexts, found {}",
        contexts.len()
    );
    for (expected_index, context) in contexts.iter().enumerate() {
        ensure!(
            context.chapter_index == expected_index as u8,
            "chapter-intro E5 contexts are not a contiguous 00..18 sequence"
        );
        ensure!(
            context.prefix_payload[..4] == CHAPTER_INTRO_SHARED_PAYLOAD,
            "chapter-intro E5 shared payload changed at source file offset 0x{:05X}",
            context.file_offset
        );
    }

    Ok(contexts)
}

pub(crate) fn bind_chapter_intro_lifetime_contexts(
    rom: &Rom,
) -> Result<Vec<ChapterIntroLifetimeContext>> {
    validated_chapter_intro_contexts(rom).map(|contexts| {
        contexts
            .into_iter()
            .map(|context| ChapterIntroLifetimeContext {
                chapter_index: context.chapter_index,
                canonical_entry_index: context.canonical_entry_index,
                entry_indices: context.entry_indices,
            })
            .collect()
    })
}

pub(super) fn bind_chapter_intro_contexts(rom: &Rom) -> Result<ChapterIntroContextSummary> {
    let contexts = validated_chapter_intro_contexts(rom)?;

    Ok(ChapterIntroContextSummary {
        prefix_code: 0xE5,
        prefix_code_hex: "E5",
        payload_destinations: [0x0071, 0x0070, 0x05CF, 0x05D0, CHAPTER_INDEX_ADDRESS],
        payload_destination_hex: ["0x0071", "0x0070", "0x05CF", "0x05D0", "0x781D"],
        unique_context_count: contexts.len(),
        first_chapter_index: contexts
            .first()
            .context("no chapter contexts")?
            .chapter_index,
        last_chapter_index: contexts
            .last()
            .context("no chapter contexts")?
            .chapter_index,
        chapter_index_address: CHAPTER_INDEX_ADDRESS,
        chapter_index_address_hex: "0x781D",
        shared_non_index_payload_sha1: sha1_hex(&CHAPTER_INTRO_SHARED_PAYLOAD),
        source_entry_indices: contexts
            .into_iter()
            .map(|context| context.entry_indices)
            .collect(),
    })
}

pub(super) fn bind_chapter_titles(rom: &Rom) -> Result<ChapterTitleSummary> {
    let pointer_table_file_offset = source_file_offset(0x0F, CHAPTER_TITLE_POINTER_TABLE_ADDRESS)?;
    let pointer_table_end = pointer_table_file_offset + CHAPTER_TITLE_POINTER_TABLE_BYTES.len();
    let pointers = rom.data()[pointer_table_file_offset..pointer_table_end]
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    ensure!(
        pointers.len() == CHAPTER_TITLE_COUNT,
        "chapter-title pointer count changed"
    );
    ensure!(
        pointers.windows(2).all(|pair| pair[0] < pair[1]),
        "chapter-title pointers are not strictly increasing"
    );

    let mut data_end_exclusive = CHAPTER_TITLE_DATA_START;
    let mut protected_digit_count = 0;
    for (index, pointer) in pointers.iter().copied().enumerate() {
        let file_offset = source_file_offset(0x0F, pointer)?;
        if index == 0 {
            ensure!(
                file_offset == CHAPTER_TITLE_DATA_START,
                "chapter-title data start changed"
            );
        }
        let relative_end = rom.data()[file_offset..CHAPTER_TITLE_DATA_END_EXCLUSIVE]
            .iter()
            .position(|byte| *byte == CHAPTER_TITLE_TERMINATOR)
            .with_context(|| format!("chapter-title entry {index} has no ED terminator"))?;
        let entry_end_exclusive = file_offset + relative_end + 1;
        if let Some(next_pointer) = pointers.get(index + 1) {
            ensure!(
                entry_end_exclusive == source_file_offset(0x0F, *next_pointer)?,
                "chapter-title entry {index} does not end at the next pointer"
            );
        }
        protected_digit_count += rom.data()[file_offset..entry_end_exclusive]
            .iter()
            .filter(|byte| (0x60..=0x69).contains(*byte))
            .count();
        data_end_exclusive = entry_end_exclusive;
    }
    ensure!(
        data_end_exclusive == CHAPTER_TITLE_DATA_END_EXCLUSIVE,
        "chapter-title data does not end at the next text table"
    );
    ensure!(
        protected_digit_count == CHAPTER_TITLE_DIGIT_COUNT,
        "chapter-title protected digit count changed"
    );

    Ok(ChapterTitleSummary {
        pointer_table: location(0x0F, CHAPTER_TITLE_POINTER_TABLE_ADDRESS),
        pointer_count: pointers.len(),
        data_file_start: CHAPTER_TITLE_DATA_START,
        data_file_start_hex: format!("0x{CHAPTER_TITLE_DATA_START:05X}"),
        data_file_end_exclusive: data_end_exclusive,
        data_file_end_exclusive_hex: format!("0x{data_end_exclusive:05X}"),
        source_terminator: CHAPTER_TITLE_TERMINATOR,
        source_terminator_hex: "ED",
        protected_original_digit_count: protected_digit_count,
        composer: location(0x0B, 0x88C4),
        selector_address: CHAPTER_INDEX_ADDRESS,
        selector_address_hex: "0x781D",
        translation_target: "Japanese chapter-title glyphs only; preserve original chapter-number digits",
    })
}

pub(super) fn bind_source_region(rom: &Rom, spec: SourceRegionSpec) -> Result<SourceRegionBinding> {
    let file_offset = source_file_offset(spec.prg_bank, spec.cpu_address)?;
    let byte_count = match spec.expectation {
        RegionExpectation::Bytes(bytes) => bytes.len(),
        RegionExpectation::Sha1 { byte_count, .. } => byte_count,
    };
    let end = file_offset
        .checked_add(byte_count)
        .context("chapter-transition source region overflow")?;
    let actual = rom
        .data()
        .get(file_offset..end)
        .with_context(|| format!("{} source region is outside the ROM", spec.role))?;
    match spec.expectation {
        RegionExpectation::Bytes(bytes) => {
            ensure!(actual == bytes, "{} source bytes changed", spec.role)
        }
        RegionExpectation::Sha1 { expected_sha1, .. } => ensure!(
            sha1_hex(actual) == expected_sha1,
            "{} source-region SHA-1 changed",
            spec.role
        ),
    }
    let typed_instructions = match spec.kind {
        RegionKind::Code => decode_rp2a03_sequence(actual, spec.cpu_address, spec.role)?,
        RegionKind::Data => Vec::new(),
    };

    Ok(SourceRegionBinding {
        role: spec.role,
        region_kind: match spec.kind {
            RegionKind::Code => "rp2a03_code",
            RegionKind::Data => "data",
        },
        prg_bank: spec.prg_bank,
        prg_bank_hex: format!("0x{:02X}", spec.prg_bank),
        cpu_address: spec.cpu_address,
        cpu_address_hex: format!("0x{:04X}", spec.cpu_address),
        file_offset,
        file_offset_hex: format!("0x{file_offset:05X}"),
        byte_count,
        source_sha1: sha1_hex(actual),
        typed_instructions,
    })
}

pub(super) fn source_file_offset(prg_bank: u8, cpu_address: u16) -> Result<usize> {
    let bank_offset = if prg_bank == 0x0F {
        ensure!(
            cpu_address >= FIXED_CPU_START,
            "fixed-bank address is below 0xC000"
        );
        usize::from(cpu_address - FIXED_CPU_START)
    } else {
        ensure!(
            (SWITCHABLE_CPU_START..FIXED_CPU_START).contains(&cpu_address),
            "switchable-bank address is outside 0x8000..0xBFFF"
        );
        usize::from(cpu_address - SWITCHABLE_CPU_START)
    };
    Ok(HEADER_SIZE + usize::from(prg_bank) * PRG_BANK_SIZE + bank_offset)
}

pub(super) fn location(prg_bank: u8, cpu_address: u16) -> CodeLocation {
    CodeLocation {
        prg_bank,
        prg_bank_hex: format!("0x{prg_bank:02X}"),
        cpu_address,
        cpu_address_hex: format!("0x{cpu_address:04X}"),
    }
}
