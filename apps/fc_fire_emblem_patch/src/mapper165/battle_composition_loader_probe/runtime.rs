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
    build_runtime_routines_for_layout(
        directories,
        PROBE_RUNTIME_LAYOUT,
        SOURCE_PAIR_AWARE_RIGHT_FD_SELECTOR,
    )
}

pub(crate) fn build_runtime_routines_for_layout(
    directories: RecipeDirectoryAddresses,
    layout: BattleCompositionRuntimeLayout,
    central_fallback_target: u16,
) -> Result<Vec<RuntimeRoutine>> {
    let routines = vec![
        RuntimeRoutine {
            role: "NMI post-mask composition dispatch",
            address: layout.dispatch,
            bytes: composition_dispatch_for_layout(layout)?,
        },
        RuntimeRoutine {
            role: "source-page restore and recipe composition",
            address: layout.compose_page,
            bytes: compose_page_for_layout(directories, layout)?,
        },
        RuntimeRoutine {
            role: "recipe payload application",
            address: layout.apply_recipe,
            bytes: apply_recipe_for_layout(layout)?,
        },
        RuntimeRoutine {
            role: "recipe directory lookup",
            address: layout.apply_directory,
            bytes: apply_directory_entry_for_layout(layout)?,
        },
        RuntimeRoutine {
            role: "participant recipe dispatch",
            address: layout.apply_participant,
            bytes: apply_participant_for_layout(directories, layout)?,
        },
        RuntimeRoutine {
            role: "source-bound dialogue selector projection",
            address: layout.project_dialogue_selector,
            bytes: project_dialogue_selector_for_layout(layout)?,
        },
        RuntimeRoutine {
            role: "gameplay and sound-test battle-surface predicate",
            address: layout.battle_surface_active,
            bytes: battle_surface_active_for_layout(layout)?,
        },
        RuntimeRoutine {
            role: "sound-test battle remap-state initializer",
            address: layout.initialize_sound_test_battle_remap,
            bytes: initialize_sound_test_battle_remap_for_layout(layout)?,
        },
        RuntimeRoutine {
            role: "battle-exit remap-state clear",
            address: layout.clear_remap_state_after_battle,
            bytes: clear_remap_state_after_battle_for_layout(layout)?,
        },
        RuntimeRoutine {
            role: "battle text projection wrapper",
            address: layout.text_projection_wrapper,
            bytes: text_projection_wrapper_for_layout(layout)?,
        },
        RuntimeRoutine {
            role: "battle-aware direct right FD selection",
            address: layout.battle_right_fd_selector,
            bytes: battle_right_selector_for_layout(layout.battle_right_fd_selector, 2, layout)?,
        },
        RuntimeRoutine {
            role: "battle-aware central right FD selection",
            address: layout.battle_central_right_fd_selector,
            bytes: battle_central_right_fd_selector_for_layout(layout, central_fallback_target)?,
        },
        RuntimeRoutine {
            role: "battle-aware right FE selection",
            address: layout.battle_right_fe_selector,
            bytes: battle_right_selector_for_layout(layout.battle_right_fe_selector, 4, layout)?,
        },
        RuntimeRoutine {
            role: "abstract-color remap projection",
            address: layout.project_color,
            bytes: project_color_for_layout(layout)?,
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
        last.address as usize + last.bytes.len() <= layout.fixed_cave_end as usize,
        "battle composition runtime reaches fixed-bank data"
    );
    Ok(routines)
}

#[cfg(test)]
pub(super) fn composition_dispatch() -> Result<Vec<u8>> {
    composition_dispatch_for_layout(PROBE_RUNTIME_LAYOUT)
}

fn composition_dispatch_for_layout(layout: BattleCompositionRuntimeLayout) -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::JsrAbsolute(SOURCE_NMI_INPUT_SCAN),
        Instruction::Php,
        Instruction::Pha,
        Instruction::Txa,
        Instruction::Pha,
        Instruction::Tya,
        Instruction::Pha,
        Instruction::JsrAbsolute(layout.battle_surface_active),
    ];
    let battle_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(layout.dispatch));
    instructions.extend([
        Instruction::LdaAbsolute(MAIN_STATE_ADDRESS),
        Instruction::JsrAbsolute(layout.clear_remap_state_after_battle),
        Instruction::JmpAbsolute(layout.dispatch),
    ]);
    let non_battle_restore_placeholder = instructions.len() - 1;
    let battle = next_address(layout.dispatch, &instructions)?;
    instructions[battle_placeholder] = Instruction::BneAbsolute(battle);
    instructions.extend([
        Instruction::LdaAbsolute(REMAP_STATE_ADDRESS),
        Instruction::AndImmediate(CACHE_UPLOADED_MARKER),
    ]);
    let uploaded_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(layout.dispatch));
    instructions.push(Instruction::LdaAbsolute(BATTLE_ACTIVE_FLAG));
    let inactive_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(layout.dispatch));
    instructions.extend([
        Instruction::LdaZeroPage(PPU_MASK_SHADOW),
        Instruction::CmpImmediate(UPLOAD_RENDER_MASK),
    ]);
    let wrong_render_state_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(layout.dispatch));
    instructions.extend([
        Instruction::JsrAbsolute(layout.compose_page),
        Instruction::JsrAbsolute(SOURCE_NMI_SCROLL_RESTORE),
    ]);
    let restore = next_address(layout.dispatch, &instructions)?;
    instructions[non_battle_restore_placeholder] = Instruction::JmpAbsolute(restore);
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
    assemble_at(layout.dispatch, &instructions)
}

#[cfg(test)]
pub(super) fn compose_page(directories: RecipeDirectoryAddresses) -> Result<Vec<u8>> {
    compose_page_for_layout(directories, PROBE_RUNTIME_LAYOUT)
}

fn compose_page_for_layout(
    directories: RecipeDirectoryAddresses,
    layout: BattleCompositionRuntimeLayout,
) -> Result<Vec<u8>> {
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
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::LdaImmediate(GLYPH_ATLAS_MMC3_PAGE),
        Instruction::StaAbsolute(0x8001),
        Instruction::LdaImmediate(7),
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::LdaImmediate(SOURCE_PAGE_MMC3_PAGE),
        Instruction::StaAbsolute(0x8001),
        Instruction::JsrAbsolute(DYNAMIC_ASSIGNMENT_CODE_CPU_ADDRESS),
    ]);
    let assignment_succeeded_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(layout.compose_page));
    let assignment_failed_placeholder = instructions.len();
    instructions.push(Instruction::JmpAbsolute(layout.compose_page));
    let assignment_succeeded = next_address(layout.compose_page, &instructions)?;
    instructions[assignment_succeeded_placeholder] = Instruction::BeqAbsolute(assignment_succeeded);
    instructions.extend([
        Instruction::LdaImmediate(2),
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(0x8001),
        Instruction::LdaImmediate(4),
        crate::mapper165::selector_safety::select_register_instruction(),
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
    let source_copy_loop = next_address(layout.compose_page, &instructions)?;
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
        Instruction::JsrAbsolute(layout.apply_recipe),
        Instruction::LdaAbsolute(0x0304),
        Instruction::JsrAbsolute(layout.apply_participant),
        Instruction::LdaAbsolute(0x0305),
        Instruction::JsrAbsolute(layout.apply_participant),
    ]);
    set_directory(&mut instructions, directories.class);
    instructions.extend([
        Instruction::LdaAbsolute(0x0306),
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::JsrAbsolute(layout.apply_directory),
        Instruction::LdaAbsolute(0x0307),
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::JsrAbsolute(layout.apply_directory),
    ]);
    set_directory(&mut instructions, directories.item);
    instructions.extend([
        Instruction::LdaAbsolute(0x0320),
        Instruction::JsrAbsolute(layout.apply_directory),
        Instruction::LdaAbsolute(0x0321),
        Instruction::JsrAbsolute(layout.apply_directory),
    ]);
    set_directory(&mut instructions, directories.terrain);
    instructions.extend([
        Instruction::LdaAbsolute(0x0322),
        Instruction::JsrAbsolute(layout.apply_directory),
        Instruction::LdaAbsolute(0x0323),
        Instruction::JsrAbsolute(layout.apply_directory),
    ]);
    set_directory(&mut instructions, directories.dialogue);
    instructions.extend([
        Instruction::JsrAbsolute(layout.project_dialogue_selector),
        Instruction::JsrAbsolute(layout.apply_directory),
        Instruction::LdaAbsolute(REMAP_STATE_ADDRESS),
        Instruction::OraImmediate(CACHE_UPLOADED_MARKER),
        Instruction::StaAbsolute(REMAP_STATE_ADDRESS),
    ]);
    let restore = next_address(layout.compose_page, &instructions)?;
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
    assemble_at(layout.compose_page, &instructions)
}

#[cfg(test)]
pub(super) fn clear_remap_state_after_battle() -> Result<Vec<u8>> {
    clear_remap_state_after_battle_for_layout(PROBE_RUNTIME_LAYOUT)
}

fn clear_remap_state_after_battle_for_layout(
    layout: BattleCompositionRuntimeLayout,
) -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::CmpImmediate(PLAYER_INITIATED_BATTLE_STATE + 1),
        Instruction::BeqAbsolute(layout.clear_remap_state_after_battle),
        Instruction::CmpImmediate(ENEMY_INITIATED_BATTLE_STATE + 1),
    ];
    let not_exit_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(
        layout.clear_remap_state_after_battle,
    ));
    let clear = next_address(layout.clear_remap_state_after_battle, &instructions)?;
    instructions[1] = Instruction::BeqAbsolute(clear);
    instructions.extend([
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(REMAP_STATE_ADDRESS),
    ]);
    let done = next_address(layout.clear_remap_state_after_battle, &instructions)?;
    instructions[not_exit_placeholder] = Instruction::BneAbsolute(done);
    instructions.push(Instruction::Rts);
    assemble_at(layout.clear_remap_state_after_battle, &instructions)
}

#[cfg(test)]
pub(super) fn apply_recipe() -> Result<Vec<u8>> {
    apply_recipe_for_layout(PROBE_RUNTIME_LAYOUT)
}

fn apply_recipe_for_layout(layout: BattleCompositionRuntimeLayout) -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(RECIPE_POINTER_LOW),
    ];
    let done_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(layout.apply_recipe));
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
    let pair_loop = next_address(layout.apply_recipe, &instructions)?;
    instructions.extend([
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(RECIPE_POINTER_LOW),
        Instruction::Tax,
        Instruction::LdaAbsoluteX(PHYSICAL_CODE_TABLE_CPU_ADDRESS),
        Instruction::JsrAbsolute(layout.project_color),
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
    let tile_copy_loop = next_address(layout.apply_recipe, &instructions)?;
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
    let done = next_address(layout.apply_recipe, &instructions)? - 1;
    instructions[done_placeholder] = Instruction::BeqAbsolute(done);
    assemble_at(layout.apply_recipe, &instructions)
}

fn apply_directory_entry_for_layout(layout: BattleCompositionRuntimeLayout) -> Result<Vec<u8>> {
    assemble_at(
        layout.apply_directory,
        &[
            Instruction::AslAccumulator,
            Instruction::Tay,
            Instruction::LdaIndirectY(DIRECTORY_POINTER_LOW),
            Instruction::StaZeroPage(RECIPE_POINTER_LOW),
            Instruction::Iny,
            Instruction::LdaIndirectY(DIRECTORY_POINTER_LOW),
            Instruction::OraImmediate(0xB0),
            Instruction::StaZeroPage(RECIPE_POINTER_HIGH),
            Instruction::JmpAbsolute(layout.apply_recipe),
        ],
    )
}

#[cfg(test)]
pub(super) fn apply_participant(directories: RecipeDirectoryAddresses) -> Result<Vec<u8>> {
    apply_participant_for_layout(directories, PROBE_RUNTIME_LAYOUT)
}

fn apply_participant_for_layout(
    directories: RecipeDirectoryAddresses,
    layout: BattleCompositionRuntimeLayout,
) -> Result<Vec<u8>> {
    let unit = layout.apply_participant + 24;
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
        Instruction::JmpAbsolute(layout.apply_directory),
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
        Instruction::JmpAbsolute(layout.apply_directory),
    ]);
    assemble_at(layout.apply_participant, &instructions)
}

fn project_dialogue_selector_for_layout(layout: BattleCompositionRuntimeLayout) -> Result<Vec<u8>> {
    let observed = layout.project_dialogue_selector + 23;
    assemble_at(
        layout.project_dialogue_selector,
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

#[cfg(test)]
pub(super) fn battle_surface_active() -> Result<Vec<u8>> {
    battle_surface_active_for_layout(PROBE_RUNTIME_LAYOUT)
}

pub(super) fn battle_surface_active_for_layout(
    layout: BattleCompositionRuntimeLayout,
) -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::LdaAbsolute(MAIN_STATE_ADDRESS),
        Instruction::CmpImmediate(PLAYER_INITIATED_BATTLE_STATE),
    ];
    let player_battle_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(layout.battle_surface_active));
    instructions.push(Instruction::CmpImmediate(ENEMY_INITIATED_BATTLE_STATE));
    let enemy_battle_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(layout.battle_surface_active));
    instructions.push(Instruction::CmpImmediate(SOUND_TEST_MAIN_STATE));
    let inactive_main_state_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(layout.battle_surface_active));
    instructions.extend([
        Instruction::LdaAbsolute(DIALOGUE_SUBSTATE_ADDRESS),
        Instruction::CmpImmediate(SOUND_TEST_BATTLE_SUBSTATE),
    ]);
    let inactive_substate_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(layout.battle_surface_active));
    instructions.extend([
        Instruction::LdaAbsolute(SOUND_TEST_BATTLE_PHASE_ADDRESS),
        Instruction::CmpImmediate(SOUND_TEST_SHARED_BATTLE_PHASE),
    ]);
    let inactive_phase_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(layout.battle_surface_active));
    let active = next_address(layout.battle_surface_active, &instructions)?;
    instructions[player_battle_placeholder] = Instruction::BeqAbsolute(active);
    instructions[enemy_battle_placeholder] = Instruction::BeqAbsolute(active);
    instructions.extend([Instruction::LdaImmediate(1), Instruction::Rts]);
    let inactive = next_address(layout.battle_surface_active, &instructions)?;
    instructions[inactive_main_state_placeholder] = Instruction::BneAbsolute(inactive);
    instructions[inactive_substate_placeholder] = Instruction::BneAbsolute(inactive);
    instructions[inactive_phase_placeholder] = Instruction::BneAbsolute(inactive);
    instructions.extend([Instruction::LdaImmediate(0), Instruction::Rts]);
    assemble_at(layout.battle_surface_active, &instructions)
}

#[cfg(test)]
pub(super) fn initialize_sound_test_battle_remap() -> Result<Vec<u8>> {
    initialize_sound_test_battle_remap_for_layout(PROBE_RUNTIME_LAYOUT)
}

fn initialize_sound_test_battle_remap_for_layout(
    layout: BattleCompositionRuntimeLayout,
) -> Result<Vec<u8>> {
    assemble_at(
        layout.initialize_sound_test_battle_remap,
        &[
            Instruction::StaAbsolute(BATTLE_ACTIVE_FLAG),
            Instruction::Php,
            Instruction::Pha,
            Instruction::LdaImmediate(0),
            Instruction::StaAbsolute(REMAP_STATE_ADDRESS),
            Instruction::Pla,
            Instruction::Plp,
            Instruction::Rts,
        ],
    )
}

#[cfg(test)]
pub(super) fn text_projection_wrapper() -> Result<Vec<u8>> {
    text_projection_wrapper_for_layout(PROBE_RUNTIME_LAYOUT)
}

fn text_projection_wrapper_for_layout(layout: BattleCompositionRuntimeLayout) -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::Txa,
        Instruction::Pha,
        Instruction::LdaIndirectY(RECIPE_POINTER_LOW),
        Instruction::StaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::JsrAbsolute(layout.battle_surface_active),
    ];
    let natural_state_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(layout.text_projection_wrapper));
    instructions.extend([
        Instruction::LdaAbsolute(REMAP_STATE_ADDRESS),
        Instruction::AndImmediate(CACHE_UPLOADED_MARKER),
    ]);
    let natural_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(layout.text_projection_wrapper));
    instructions.extend([
        Instruction::LdaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::JsrAbsolute(layout.project_color),
        Instruction::StaZeroPage(PHYSICAL_TILE_CODE),
    ]);
    let natural = next_address(layout.text_projection_wrapper, &instructions)?;
    instructions[natural_state_placeholder] = Instruction::BeqAbsolute(natural);
    instructions[natural_placeholder] = Instruction::BeqAbsolute(natural);
    instructions.extend([
        Instruction::Pla,
        Instruction::Tax,
        Instruction::LdaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::CmpImmediate(0xEF),
        Instruction::Rts,
    ]);
    assemble_at(layout.text_projection_wrapper, &instructions)
}

fn project_color_for_layout(layout: BattleCompositionRuntimeLayout) -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::StaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::LdaAbsolute(REMAP_STATE_ADDRESS),
        Instruction::AndImmediate(REMAP_PAIR_COUNT_MASK),
        Instruction::Tax,
    ];
    let empty_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(layout.project_color));
    let loop_address = next_address(layout.project_color, &instructions)?;
    instructions.extend([
        Instruction::Dex,
        Instruction::Dex,
        Instruction::LdaAbsoluteX(REMAP_PAIR_TABLE_ADDRESS),
        Instruction::CmpZeroPage(PHYSICAL_TILE_CODE),
    ]);
    let matched_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(layout.project_color));
    instructions.extend([Instruction::Txa, Instruction::BneAbsolute(loop_address)]);
    let done = next_address(layout.project_color, &instructions)?;
    instructions[empty_placeholder] = Instruction::BeqAbsolute(done);
    instructions.extend([
        Instruction::LdaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::Rts,
    ]);
    let matched = next_address(layout.project_color, &instructions)?;
    instructions[matched_placeholder] = Instruction::BeqAbsolute(matched);
    instructions.extend([
        Instruction::LdaAbsoluteX(REMAP_PAIR_TABLE_ADDRESS + 1),
        Instruction::Rts,
    ]);
    assemble_at(layout.project_color, &instructions)
}

#[cfg(test)]
pub(super) fn battle_right_selector(address: u16, mapper_register: u8) -> Result<Vec<u8>> {
    battle_right_selector_for_layout(address, mapper_register, PROBE_RUNTIME_LAYOUT)
}

fn battle_right_selector_for_layout(
    address: u16,
    mapper_register: u8,
    layout: BattleCompositionRuntimeLayout,
) -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::Php,
        Instruction::Pha,
        Instruction::JsrAbsolute(layout.battle_surface_active),
    ];
    let inactive_surface_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(address));
    instructions.extend([
        Instruction::LdaAbsolute(REMAP_STATE_ADDRESS),
        Instruction::AndImmediate(CACHE_UPLOADED_MARKER),
    ]);
    let cache_missing_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(address));
    instructions.extend([
        Instruction::Pla,
        Instruction::Pha,
        Instruction::AndImmediate(0x1F),
    ]);
    let nonzero_page_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(address));
    instructions.extend([
        Instruction::LdaImmediate(0),
        Instruction::JmpAbsolute(address),
    ]);
    let select_register_placeholder = instructions.len() - 1;
    let natural = next_address(address, &instructions)?;
    instructions[inactive_surface_placeholder] = Instruction::BeqAbsolute(natural);
    instructions[cache_missing_placeholder] = Instruction::BeqAbsolute(natural);
    instructions[nonzero_page_placeholder] = Instruction::BneAbsolute(natural);
    instructions.extend([
        Instruction::Pla,
        Instruction::Pha,
        Instruction::AndImmediate(0x1F),
        Instruction::AslAccumulator,
        Instruction::AslAccumulator,
        Instruction::Clc,
        Instruction::AdcImmediate(8),
    ]);
    let select_register = next_address(address, &instructions)?;
    instructions[select_register_placeholder] = Instruction::JmpAbsolute(select_register);
    instructions.extend([
        Instruction::Pha,
        Instruction::LdaImmediate(mapper_register),
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::Pla,
    ]);
    instructions.extend([
        Instruction::StaAbsolute(0x8001),
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ]);
    assemble_at(address, &instructions)
}

#[cfg(test)]
pub(super) fn battle_central_right_fd_selector() -> Result<Vec<u8>> {
    battle_central_right_fd_selector_for_layout(
        PROBE_RUNTIME_LAYOUT,
        SOURCE_PAIR_AWARE_RIGHT_FD_SELECTOR,
    )
}

fn battle_central_right_fd_selector_for_layout(
    layout: BattleCompositionRuntimeLayout,
    fallback_target: u16,
) -> Result<Vec<u8>> {
    let address = layout.battle_central_right_fd_selector;
    let mut instructions = vec![
        Instruction::Php,
        Instruction::Pha,
        Instruction::JsrAbsolute(layout.battle_surface_active),
    ];
    let inactive_surface_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(address));
    instructions.extend([
        Instruction::LdaAbsolute(REMAP_STATE_ADDRESS),
        Instruction::AndImmediate(CACHE_UPLOADED_MARKER),
    ]);
    let cache_missing_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(address));
    instructions.extend([
        Instruction::Pla,
        Instruction::Pha,
        Instruction::AndImmediate(0x1F),
    ]);
    let nonzero_page_placeholder = instructions.len();
    instructions.push(Instruction::BneAbsolute(address));
    instructions.extend([
        Instruction::LdaImmediate(2),
        crate::mapper165::selector_safety::select_register_instruction(),
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(0x8001),
        Instruction::Pla,
        Instruction::Plp,
        Instruction::Rts,
    ]);
    let natural = next_address(address, &instructions)?;
    instructions[inactive_surface_placeholder] = Instruction::BeqAbsolute(natural);
    instructions[cache_missing_placeholder] = Instruction::BeqAbsolute(natural);
    instructions[nonzero_page_placeholder] = Instruction::BneAbsolute(natural);
    instructions.extend([
        Instruction::Pla,
        Instruction::Plp,
        Instruction::JmpAbsolute(fallback_target),
    ]);
    assemble_at(address, &instructions)
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
