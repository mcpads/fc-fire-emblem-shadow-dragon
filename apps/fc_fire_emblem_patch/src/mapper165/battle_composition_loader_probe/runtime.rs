use super::*;

pub(super) struct RuntimeRoutine {
    pub(super) role: &'static str,
    pub(super) address: u16,
    pub(super) bytes: Vec<u8>,
}

#[derive(Clone, Copy)]
pub(super) struct RecipeDirectoryAddresses {
    pub(super) unit: u16,
    pub(super) enemy: u16,
    pub(super) class: u16,
    pub(super) item: u16,
    pub(super) terrain: u16,
    pub(super) dialogue: u16,
}

pub(super) fn parse_recipe_directories(bytes: &[u8]) -> Result<RecipeDirectoryAddresses> {
    ensure!(
        bytes.len() >= 32
            && &bytes[..4] == b"FBRC"
            && bytes[4] == 1
            && bytes[30] == 3
            && usize::from(read_u16(bytes, 8)) == bytes.len(),
        "battle composition recipe header changed"
    );
    let address = |header_offset| -> Result<u16> {
        MATERIAL_RECIPE_CPU_ADDRESS
            .checked_add(read_u16(bytes, header_offset))
            .context("battle recipe directory CPU address overflow")
    };
    Ok(RecipeDirectoryAddresses {
        unit: address(16)?,
        enemy: address(18)?,
        class: address(20)?,
        item: address(22)?,
        terrain: address(24)?,
        dialogue: address(26)?,
    })
}

pub(super) fn build_runtime_routines(
    directories: RecipeDirectoryAddresses,
) -> Result<Vec<RuntimeRoutine>> {
    let routines = vec![
        RuntimeRoutine {
            role: "NMI post-mask composition dispatch",
            address: DISPATCH_ADDRESS,
            bytes: composition_dispatch()?,
        },
        RuntimeRoutine {
            role: "source-page restore and recipe composition",
            address: COMPOSE_PAGE_ADDRESS,
            bytes: compose_page(directories)?,
        },
        RuntimeRoutine {
            role: "recipe payload application",
            address: APPLY_RECIPE_ADDRESS,
            bytes: apply_recipe()?,
        },
        RuntimeRoutine {
            role: "recipe directory lookup",
            address: APPLY_DIRECTORY_ADDRESS,
            bytes: apply_directory_entry()?,
        },
        RuntimeRoutine {
            role: "participant recipe dispatch",
            address: APPLY_PARTICIPANT_ADDRESS,
            bytes: apply_participant(directories)?,
        },
        RuntimeRoutine {
            role: "source-bound dialogue selector projection",
            address: PROJECT_DIALOGUE_SELECTOR_ADDRESS,
            bytes: project_dialogue_selector()?,
        },
        RuntimeRoutine {
            role: "battle-aware direct right FD selection",
            address: BATTLE_RIGHT_FD_SELECTOR_ADDRESS,
            bytes: battle_right_selector(BATTLE_RIGHT_FD_SELECTOR_ADDRESS, 2)?,
        },
        RuntimeRoutine {
            role: "battle-aware central right FD selection",
            address: BATTLE_CENTRAL_RIGHT_FD_SELECTOR_ADDRESS,
            bytes: battle_central_right_fd_selector()?,
        },
        RuntimeRoutine {
            role: "battle-aware right FE selection",
            address: BATTLE_RIGHT_FE_SELECTOR_ADDRESS,
            bytes: battle_right_selector(BATTLE_RIGHT_FE_SELECTOR_ADDRESS, 4)?,
        },
        RuntimeRoutine {
            role: "battle text projection wrapper",
            address: TEXT_PROJECTION_WRAPPER_ADDRESS,
            bytes: text_projection_wrapper()?,
        },
        RuntimeRoutine {
            role: "abstract-color remap projection",
            address: PROJECT_COLOR_ADDRESS,
            bytes: project_color()?,
        },
    ];
    for pair in routines.windows(2) {
        ensure!(
            pair[0].address as usize + pair[0].bytes.len() <= pair[1].address as usize,
            "battle composition {} routine ends at {:04X} and overlaps {} at {:04X}",
            pair[0].role,
            pair[0].address as usize + pair[0].bytes.len(),
            pair[1].role,
            pair[1].address,
        );
    }
    let last = routines
        .last()
        .context("battle composition has no runtime routines")?;
    ensure!(
        last.address as usize + last.bytes.len() <= FIXED_CAVE_END_ADDRESS as usize,
        "battle composition runtime reaches fixed-bank data"
    );
    Ok(routines)
}

pub(super) fn composition_dispatch() -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::JsrAbsolute(SOURCE_NMI_INPUT_SCAN),
        Instruction::Php,
        Instruction::Pha,
        Instruction::Txa,
        Instruction::Pha,
        Instruction::Tya,
        Instruction::Pha,
        Instruction::LdaAbsolute(BATTLE_ACTIVE_FLAG),
    ];
    let inactive_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(DISPATCH_ADDRESS));
    instructions.extend([Instruction::AndImmediate(CACHE_UPLOADED_MARKER)]);
    let uploaded_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(DISPATCH_ADDRESS));
    instructions.extend([
        Instruction::LdaZeroPage(PPU_MASK_SHADOW),
        Instruction::CmpImmediate(UPLOAD_RENDER_MASK),
    ]);
    let wrong_render_state_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(DISPATCH_ADDRESS));
    instructions.extend([
        Instruction::JsrAbsolute(COMPOSE_PAGE_ADDRESS),
        Instruction::JsrAbsolute(SOURCE_NMI_SCROLL_RESTORE),
    ]);
    let restore = next_address(DISPATCH_ADDRESS, &instructions)?;
    instructions[inactive_placeholder] = Instruction::BeqAbsolute(restore);
    instructions[uploaded_placeholder] = Instruction::BneAbsolute(restore);
    instructions[wrong_render_state_placeholder] = Instruction::BneAbsolute(restore);
    instructions.extend([
        Instruction::Pla,
        Instruction::Tay,
        Instruction::Pla,
        Instruction::Tax,
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ]);
    assemble_at(DISPATCH_ADDRESS, &instructions)
}

pub(super) fn compose_page(directories: RecipeDirectoryAddresses) -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::Php,
        Instruction::Pha,
        Instruction::Txa,
        Instruction::Pha,
        Instruction::Tya,
        Instruction::Pha,
    ];
    for address in BORROWED_SCRATCH {
        instructions.extend([Instruction::LdaZeroPage(address), Instruction::Pha]);
    }
    instructions.extend([
        Instruction::LdaAbsolute(PPU_CONTROL_SHADOW),
        Instruction::Pha,
        Instruction::AndImmediate(0x7B),
        Instruction::StaAbsolute(PPU_CONTROL_SHADOW),
        Instruction::StaAbsolute(0x2000),
        Instruction::LdaImmediate(6),
        Instruction::StaAbsolute(0x8000),
        Instruction::LdaImmediate(GLYPH_ATLAS_MMC3_PAGE),
        Instruction::StaAbsolute(0x8001),
        Instruction::LdaImmediate(7),
        Instruction::StaAbsolute(0x8000),
        Instruction::LdaImmediate(SOURCE_PAGE_MMC3_PAGE),
        Instruction::StaAbsolute(0x8001),
        Instruction::JsrAbsolute(DYNAMIC_ASSIGNMENT_CODE_CPU_ADDRESS),
    ]);
    let assignment_succeeded_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(COMPOSE_PAGE_ADDRESS));
    let assignment_failed_placeholder = instructions.len();
    instructions.push(Instruction::JmpAbsolute(COMPOSE_PAGE_ADDRESS));
    let assignment_succeeded = next_address(COMPOSE_PAGE_ADDRESS, &instructions)?;
    instructions[assignment_succeeded_placeholder] = Instruction::BeqAbsolute(assignment_succeeded);
    instructions.extend([
        Instruction::LdaImmediate(2),
        Instruction::StaAbsolute(0x8000),
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(0x8001),
        Instruction::LdaImmediate(4),
        Instruction::StaAbsolute(0x8000),
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(0x8001),
        Instruction::LdaAbsolute(0x2002),
        Instruction::LdaImmediate(0x10),
        Instruction::StaAbsolute(0x2006),
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(0x2006),
        Instruction::LdaImmediate(0),
        Instruction::StaZeroPage(RECIPE_POINTER_LOW),
        Instruction::LdaImmediate(0xA0),
        Instruction::StaZeroPage(RECIPE_POINTER_HIGH),
        Instruction::LdxImmediate(16),
        Instruction::LdyImmediate(0),
    ]);
    let source_copy_loop = next_address(COMPOSE_PAGE_ADDRESS, &instructions)?;
    instructions.extend([
        Instruction::LdaIndirectY(RECIPE_POINTER_LOW),
        Instruction::StaAbsolute(0x2007),
        Instruction::Iny,
        Instruction::BneAbsolute(source_copy_loop),
        Instruction::IncAbsolute(u16::from(RECIPE_POINTER_HIGH)),
        Instruction::Dex,
        Instruction::BneAbsolute(source_copy_loop),
        Instruction::LdaAbsolute(MATERIAL_RECIPE_CPU_ADDRESS + 14),
        Instruction::StaZeroPage(RECIPE_POINTER_LOW),
        Instruction::LdaAbsolute(MATERIAL_RECIPE_CPU_ADDRESS + 15),
        Instruction::OraImmediate(0xB0),
        Instruction::StaZeroPage(RECIPE_POINTER_HIGH),
        Instruction::JsrAbsolute(APPLY_RECIPE_ADDRESS),
        Instruction::LdaAbsolute(0x0304),
        Instruction::JsrAbsolute(APPLY_PARTICIPANT_ADDRESS),
        Instruction::LdaAbsolute(0x0305),
        Instruction::JsrAbsolute(APPLY_PARTICIPANT_ADDRESS),
    ]);
    set_directory(&mut instructions, directories.class);
    instructions.extend([
        Instruction::LdaAbsolute(0x0306),
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::JsrAbsolute(APPLY_DIRECTORY_ADDRESS),
        Instruction::LdaAbsolute(0x0307),
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::JsrAbsolute(APPLY_DIRECTORY_ADDRESS),
    ]);
    set_directory(&mut instructions, directories.item);
    instructions.extend([
        Instruction::LdaAbsolute(0x0320),
        Instruction::JsrAbsolute(APPLY_DIRECTORY_ADDRESS),
        Instruction::LdaAbsolute(0x0321),
        Instruction::JsrAbsolute(APPLY_DIRECTORY_ADDRESS),
    ]);
    set_directory(&mut instructions, directories.terrain);
    instructions.extend([
        Instruction::LdaAbsolute(0x0322),
        Instruction::JsrAbsolute(APPLY_DIRECTORY_ADDRESS),
        Instruction::LdaAbsolute(0x0323),
        Instruction::JsrAbsolute(APPLY_DIRECTORY_ADDRESS),
    ]);
    set_directory(&mut instructions, directories.dialogue);
    instructions.extend([
        Instruction::JsrAbsolute(PROJECT_DIALOGUE_SELECTOR_ADDRESS),
        Instruction::JsrAbsolute(APPLY_DIRECTORY_ADDRESS),
        Instruction::LdaAbsolute(BATTLE_ACTIVE_FLAG),
        Instruction::OraImmediate(CACHE_UPLOADED_MARKER),
        Instruction::StaAbsolute(BATTLE_ACTIVE_FLAG),
    ]);
    let restore = next_address(COMPOSE_PAGE_ADDRESS, &instructions)?;
    instructions[assignment_failed_placeholder] = Instruction::JmpAbsolute(restore);
    instructions.extend([
        Instruction::LdaZeroPage(PRG_BANK_SHADOW),
        Instruction::JsrAbsolute(SOURCE_PRG_BANK_SELECTOR),
        Instruction::LdaZeroPage(RIGHT_FE_SHADOW),
        Instruction::OraZeroPage(CHR_HIGH_BITS_SHADOW),
        Instruction::JsrAbsolute(SOURCE_RIGHT_FE_SELECTOR),
        Instruction::LdaAbsolute(0x2002),
        Instruction::Pla,
        Instruction::StaAbsolute(PPU_CONTROL_SHADOW),
        Instruction::StaAbsolute(0x2000),
    ]);
    for address in BORROWED_SCRATCH.into_iter().rev() {
        instructions.extend([Instruction::Pla, Instruction::StaZeroPage(address)]);
    }
    instructions.extend([
        Instruction::Pla,
        Instruction::Tay,
        Instruction::Pla,
        Instruction::Tax,
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ]);
    assemble_at(COMPOSE_PAGE_ADDRESS, &instructions)
}

pub(super) fn apply_recipe() -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(RECIPE_POINTER_LOW),
    ];
    let done_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(APPLY_RECIPE_ADDRESS));
    instructions.extend([
        Instruction::StaAbsolute(RECIPE_PAIR_COUNT),
        Instruction::Clc,
        Instruction::LdaZeroPage(RECIPE_POINTER_LOW),
        Instruction::AdcImmediate(1),
        Instruction::StaZeroPage(RECIPE_POINTER_LOW),
        Instruction::LdaZeroPage(RECIPE_POINTER_HIGH),
        Instruction::AdcImmediate(0),
        Instruction::StaZeroPage(RECIPE_POINTER_HIGH),
    ]);
    let pair_loop = next_address(APPLY_RECIPE_ADDRESS, &instructions)?;
    instructions.extend([
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(RECIPE_POINTER_LOW),
        Instruction::Tax,
        Instruction::LdaAbsoluteX(PHYSICAL_CODE_TABLE_CPU_ADDRESS),
        Instruction::JsrAbsolute(PROJECT_COLOR_ADDRESS),
        Instruction::StaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::LdaAbsolute(0x2002),
        Instruction::LdaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::LsrAccumulator,
        Instruction::OraImmediate(0x10),
        Instruction::StaAbsolute(0x2006),
        Instruction::LdaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::StaAbsolute(0x2006),
        Instruction::LdyImmediate(1),
        Instruction::LdaIndirectY(RECIPE_POINTER_LOW),
        Instruction::StaZeroPage(ATLAS_POINTER_LOW),
        Instruction::Iny,
        Instruction::LdaIndirectY(RECIPE_POINTER_LOW),
        Instruction::StaZeroPage(ATLAS_POINTER_HIGH),
    ]);
    for _ in 0..4 {
        instructions.extend([
            Instruction::AslZeroPage(ATLAS_POINTER_LOW),
            Instruction::RolZeroPage(ATLAS_POINTER_HIGH),
        ]);
    }
    instructions.extend([
        Instruction::LdaZeroPage(ATLAS_POINTER_HIGH),
        Instruction::OraImmediate(0x80),
        Instruction::StaZeroPage(ATLAS_POINTER_HIGH),
        Instruction::LdyImmediate(0),
    ]);
    let tile_copy_loop = next_address(APPLY_RECIPE_ADDRESS, &instructions)?;
    instructions.extend([
        Instruction::LdaIndirectY(ATLAS_POINTER_LOW),
        Instruction::StaAbsolute(0x2007),
        Instruction::Iny,
        Instruction::CpyImmediate(16),
        Instruction::BneAbsolute(tile_copy_loop),
        Instruction::Clc,
        Instruction::LdaZeroPage(RECIPE_POINTER_LOW),
        Instruction::AdcImmediate(3),
        Instruction::StaZeroPage(RECIPE_POINTER_LOW),
        Instruction::LdaZeroPage(RECIPE_POINTER_HIGH),
        Instruction::AdcImmediate(0),
        Instruction::StaZeroPage(RECIPE_POINTER_HIGH),
        Instruction::DecAbsolute(RECIPE_PAIR_COUNT),
        Instruction::BneAbsolute(pair_loop),
        Instruction::Rts,
    ]);
    let done = next_address(APPLY_RECIPE_ADDRESS, &instructions)? - 1;
    instructions[done_placeholder] = Instruction::BeqAbsolute(done);
    assemble_at(APPLY_RECIPE_ADDRESS, &instructions)
}

fn apply_directory_entry() -> Result<Vec<u8>> {
    assemble_at(
        APPLY_DIRECTORY_ADDRESS,
        &[
            Instruction::AslAccumulator,
            Instruction::Tay,
            Instruction::LdaIndirectY(DIRECTORY_POINTER_LOW),
            Instruction::StaZeroPage(RECIPE_POINTER_LOW),
            Instruction::Iny,
            Instruction::LdaIndirectY(DIRECTORY_POINTER_LOW),
            Instruction::OraImmediate(0xB0),
            Instruction::StaZeroPage(RECIPE_POINTER_HIGH),
            Instruction::JmpAbsolute(APPLY_RECIPE_ADDRESS),
        ],
    )
}

pub(super) fn apply_participant(directories: RecipeDirectoryAddresses) -> Result<Vec<u8>> {
    let unit = APPLY_PARTICIPANT_ADDRESS + 24;
    let mut instructions = vec![
        Instruction::Pha,
        Instruction::CmpImmediate(0x80),
        Instruction::BccAbsolute(unit),
        Instruction::Pla,
        Instruction::AndImmediate(0x7F),
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::Tax,
    ];
    set_directory(&mut instructions, directories.enemy);
    instructions.extend([
        Instruction::Txa,
        Instruction::JmpAbsolute(APPLY_DIRECTORY_ADDRESS),
    ]);
    instructions.extend([
        Instruction::Pla,
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::Tax,
    ]);
    set_directory(&mut instructions, directories.unit);
    instructions.extend([
        Instruction::Txa,
        Instruction::JmpAbsolute(APPLY_DIRECTORY_ADDRESS),
    ]);
    assemble_at(APPLY_PARTICIPANT_ADDRESS, &instructions)
}

fn project_dialogue_selector() -> Result<Vec<u8>> {
    let observed = PROJECT_DIALOGUE_SELECTOR_ADDRESS + 23;
    assemble_at(
        PROJECT_DIALOGUE_SELECTOR_ADDRESS,
        &[
            Instruction::LdaAbsolute(0x0334),
            Instruction::BeqAbsolute(observed),
            Instruction::LdaAbsolute(0x0479),
            Instruction::BeqAbsolute(observed),
            Instruction::LdaAbsolute(0x0335),
            Instruction::BeqAbsolute(observed),
            Instruction::LdaAbsolute(0x05DF),
            Instruction::BneAbsolute(observed),
            Instruction::LdaImmediate(0x3E),
            Instruction::Rts,
            Instruction::LdaAbsolute(0x7936),
            Instruction::Rts,
        ],
    )
}

pub(super) fn text_projection_wrapper() -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::Txa,
        Instruction::Pha,
        Instruction::LdaIndirectY(RECIPE_POINTER_LOW),
        Instruction::StaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::LdaAbsolute(BATTLE_ACTIVE_FLAG),
        Instruction::AndImmediate(CACHE_UPLOADED_MARKER),
    ];
    let natural_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(TEXT_PROJECTION_WRAPPER_ADDRESS));
    instructions.extend([
        Instruction::LdaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::JsrAbsolute(PROJECT_COLOR_ADDRESS),
        Instruction::StaZeroPage(PHYSICAL_TILE_CODE),
    ]);
    let natural = next_address(TEXT_PROJECTION_WRAPPER_ADDRESS, &instructions)?;
    instructions[natural_placeholder] = Instruction::BeqAbsolute(natural);
    instructions.extend([
        Instruction::Pla,
        Instruction::Tax,
        Instruction::LdaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::CmpImmediate(0xEF),
        Instruction::Rts,
    ]);
    assemble_at(TEXT_PROJECTION_WRAPPER_ADDRESS, &instructions)
}

fn project_color() -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::StaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::LdaAbsolute(BATTLE_ACTIVE_FLAG),
        Instruction::AndImmediate(0x1E),
        Instruction::Tax,
    ];
    let empty_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(PROJECT_COLOR_ADDRESS));
    let loop_address = next_address(PROJECT_COLOR_ADDRESS, &instructions)?;
    instructions.extend([
        Instruction::Dex,
        Instruction::Dex,
        Instruction::LdaAbsoluteX(REMAP_PAIR_TABLE_ADDRESS),
        Instruction::CmpZeroPage(PHYSICAL_TILE_CODE),
    ]);
    let matched_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(PROJECT_COLOR_ADDRESS));
    instructions.extend([Instruction::Txa, Instruction::BneAbsolute(loop_address)]);
    let done = next_address(PROJECT_COLOR_ADDRESS, &instructions)?;
    instructions[empty_placeholder] = Instruction::BeqAbsolute(done);
    instructions.extend([
        Instruction::LdaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::Rts,
    ]);
    let matched = next_address(PROJECT_COLOR_ADDRESS, &instructions)?;
    instructions[matched_placeholder] = Instruction::BeqAbsolute(matched);
    instructions.extend([
        Instruction::LdaAbsoluteX(REMAP_PAIR_TABLE_ADDRESS + 1),
        Instruction::Rts,
    ]);
    assemble_at(PROJECT_COLOR_ADDRESS, &instructions)
}

pub(super) fn battle_right_selector(address: u16, mapper_register: u8) -> Result<Vec<u8>> {
    let cache_marker_test = address + 13;
    let natural = address + 31;
    let write = address + 40;
    assemble_at(
        address,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::LdaAbsolute(MAIN_STATE_ADDRESS),
            Instruction::CmpImmediate(PLAYER_INITIATED_BATTLE_STATE),
            Instruction::BeqAbsolute(cache_marker_test),
            Instruction::CmpImmediate(ENEMY_INITIATED_BATTLE_STATE),
            Instruction::BneAbsolute(natural),
            Instruction::LdaAbsolute(BATTLE_ACTIVE_FLAG),
            Instruction::AndImmediate(CACHE_UPLOADED_MARKER),
            Instruction::BeqAbsolute(natural),
            Instruction::Pla,
            Instruction::Pha,
            Instruction::AndImmediate(0x1F),
            Instruction::BneAbsolute(natural),
            Instruction::LdaImmediate(0),
            Instruction::JmpAbsolute(write),
            Instruction::Pla,
            Instruction::Pha,
            Instruction::AndImmediate(0x1F),
            Instruction::AslAccumulator,
            Instruction::AslAccumulator,
            Instruction::Clc,
            Instruction::AdcImmediate(8),
            Instruction::Pha,
            Instruction::LdaImmediate(mapper_register),
            Instruction::StaAbsolute(0x8000),
            Instruction::Pla,
            Instruction::StaAbsolute(0x8001),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

pub(super) fn battle_central_right_fd_selector() -> Result<Vec<u8>> {
    let cache_marker_test = BATTLE_CENTRAL_RIGHT_FD_SELECTOR_ADDRESS + 13;
    let natural = BATTLE_CENTRAL_RIGHT_FD_SELECTOR_ADDRESS + 39;
    assemble_at(
        BATTLE_CENTRAL_RIGHT_FD_SELECTOR_ADDRESS,
        &[
            Instruction::Php,
            Instruction::Pha,
            Instruction::LdaAbsolute(MAIN_STATE_ADDRESS),
            Instruction::CmpImmediate(PLAYER_INITIATED_BATTLE_STATE),
            Instruction::BeqAbsolute(cache_marker_test),
            Instruction::CmpImmediate(ENEMY_INITIATED_BATTLE_STATE),
            Instruction::BneAbsolute(natural),
            Instruction::LdaAbsolute(BATTLE_ACTIVE_FLAG),
            Instruction::AndImmediate(CACHE_UPLOADED_MARKER),
            Instruction::BeqAbsolute(natural),
            Instruction::Pla,
            Instruction::Pha,
            Instruction::AndImmediate(0x1F),
            Instruction::BneAbsolute(natural),
            Instruction::LdaImmediate(2),
            Instruction::StaAbsolute(0x8000),
            Instruction::LdaImmediate(0),
            Instruction::StaAbsolute(0x8001),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
            Instruction::Pla,
            Instruction::Plp,
            Instruction::JmpAbsolute(SOURCE_PAIR_AWARE_RIGHT_FD_SELECTOR),
        ],
    )
}

fn set_directory(instructions: &mut Vec<Instruction>, address: u16) {
    instructions.extend([
        Instruction::LdaImmediate(address as u8),
        Instruction::StaZeroPage(DIRECTORY_POINTER_LOW),
        Instruction::LdaImmediate((address >> 8) as u8),
        Instruction::StaZeroPage(DIRECTORY_POINTER_HIGH),
    ]);
}

fn next_address(origin: u16, instructions: &[Instruction]) -> Result<u16> {
    origin
        .checked_add(u16::try_from(assemble_at(origin, instructions)?.len())?)
        .context("battle composition routine address overflow")
}
