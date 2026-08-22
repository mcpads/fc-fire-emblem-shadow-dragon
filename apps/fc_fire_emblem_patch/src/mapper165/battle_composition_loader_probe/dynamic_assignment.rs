use super::{runtime::RecipeDirectoryAddresses, *};

const COLLECT_RECIPE_ADDRESS: u16 = 0x9700;
const COLLECT_DIRECTORY_ADDRESS: u16 = 0x9740;
const COLLECT_PARTICIPANT_NAME_ADDRESS: u16 = 0x9760;
const MARK_SELECTED_COLOR_ADDRESS: u16 = 0x97A0;
const ALLOCATE_REMAP_PAIRS_ADDRESS: u16 = 0x97D0;
const TEST_SELECTED_COLOR_ADDRESS: u16 = 0x9850;
const MATERIAL_CODE_END_ADDRESS: u16 = 0x98A0;

pub(super) struct MaterialRuntimeRoutine {
    pub(super) role: &'static str,
    pub(super) address: u16,
    pub(super) bytes: Vec<u8>,
}

pub(super) fn build_dynamic_assignment_routines(
    directories: RecipeDirectoryAddresses,
) -> Result<Vec<MaterialRuntimeRoutine>> {
    build_dynamic_assignment_routines_for_layout(directories, PROBE_RUNTIME_LAYOUT)
}

pub(crate) fn build_dynamic_assignment_routines_for_layout(
    directories: RecipeDirectoryAddresses,
    layout: BattleCompositionRuntimeLayout,
) -> Result<Vec<MaterialRuntimeRoutine>> {
    let routines = vec![
        MaterialRuntimeRoutine {
            role: "selected-color collection dispatch",
            address: DYNAMIC_ASSIGNMENT_CODE_CPU_ADDRESS,
            bytes: prepare_dynamic_assignment(directories, layout.project_dialogue_selector)?,
        },
        MaterialRuntimeRoutine {
            role: "recipe selected-color collection",
            address: COLLECT_RECIPE_ADDRESS,
            bytes: collect_recipe_colors()?,
        },
        MaterialRuntimeRoutine {
            role: "recipe directory selected-color collection",
            address: COLLECT_DIRECTORY_ADDRESS,
            bytes: collect_directory_colors()?,
        },
        MaterialRuntimeRoutine {
            role: "participant-name selected-color collection",
            address: COLLECT_PARTICIPANT_NAME_ADDRESS,
            bytes: collect_participant_name_colors()?,
        },
        MaterialRuntimeRoutine {
            role: "selected-color bitmap insertion",
            address: MARK_SELECTED_COLOR_ADDRESS,
            bytes: mark_selected_color()?,
        },
        MaterialRuntimeRoutine {
            role: "protected-color remap allocation",
            address: ALLOCATE_REMAP_PAIRS_ADDRESS,
            bytes: allocate_remap_pairs(layout.project_dialogue_selector)?,
        },
        MaterialRuntimeRoutine {
            role: "selected-color bitmap membership",
            address: TEST_SELECTED_COLOR_ADDRESS,
            bytes: test_selected_color()?,
        },
    ];
    for pair in routines.windows(2) {
        ensure!(
            pair[0].address as usize + pair[0].bytes.len() <= pair[1].address as usize,
            "battle dynamic-assignment {} routine overlaps {}",
            pair[0].role,
            pair[1].role
        );
    }
    let last = routines
        .last()
        .context("battle dynamic assignment has no material routines")?;
    ensure!(
        last.address as usize + last.bytes.len() <= MATERIAL_CODE_END_ADDRESS as usize,
        "battle dynamic-assignment runtime exceeds its material code region"
    );
    Ok(routines)
}

fn prepare_dynamic_assignment(
    directories: RecipeDirectoryAddresses,
    project_dialogue_selector_address: u16,
) -> Result<Vec<u8>> {
    let mut instructions = vec![Instruction::LdxImmediate(SELECTED_COLOR_BITMAP_BYTE_COUNT)];
    let clear_loop = next_address(DYNAMIC_ASSIGNMENT_CODE_CPU_ADDRESS, &instructions)?;
    instructions.extend([
        Instruction::Dex,
        Instruction::LdaImmediate(0),
        Instruction::StaAbsoluteX(SELECTED_COLOR_BITMAP_ADDRESS),
        Instruction::Txa,
        Instruction::BneAbsolute(clear_loop),
        Instruction::LdaImmediate(0),
        Instruction::StaAbsolute(REMAP_STATE_ADDRESS),
        Instruction::LdaAbsolute(MATERIAL_RECIPE_CPU_ADDRESS + 14),
        Instruction::StaZeroPage(RECIPE_POINTER_LOW),
        Instruction::LdaAbsolute(MATERIAL_RECIPE_CPU_ADDRESS + 15),
        Instruction::OraImmediate(0xB0),
        Instruction::StaZeroPage(RECIPE_POINTER_HIGH),
        Instruction::JsrAbsolute(COLLECT_RECIPE_ADDRESS),
    ]);
    set_directory(&mut instructions, directories.unit);
    instructions.extend([
        Instruction::LdaAbsolute(0x0304),
        Instruction::JsrAbsolute(COLLECT_PARTICIPANT_NAME_ADDRESS),
    ]);
    set_directory(&mut instructions, directories.enemy);
    instructions.extend([
        Instruction::LdaAbsolute(0x0305),
        Instruction::AndImmediate(0x7F),
        Instruction::JsrAbsolute(COLLECT_PARTICIPANT_NAME_ADDRESS),
    ]);
    set_directory(&mut instructions, directories.class);
    instructions.extend([
        Instruction::LdaAbsolute(0x0306),
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::JsrAbsolute(COLLECT_DIRECTORY_ADDRESS),
        Instruction::LdaAbsolute(0x0307),
        Instruction::Sec,
        Instruction::SbcImmediate(1),
        Instruction::JsrAbsolute(COLLECT_DIRECTORY_ADDRESS),
    ]);
    set_directory(&mut instructions, directories.item);
    instructions.extend([
        Instruction::LdaAbsolute(0x0320),
        Instruction::JsrAbsolute(COLLECT_DIRECTORY_ADDRESS),
        Instruction::LdaAbsolute(0x0321),
        Instruction::JsrAbsolute(COLLECT_DIRECTORY_ADDRESS),
    ]);
    set_directory(&mut instructions, directories.terrain);
    instructions.extend([
        Instruction::LdaAbsolute(0x0322),
        Instruction::JsrAbsolute(COLLECT_DIRECTORY_ADDRESS),
        Instruction::LdaAbsolute(0x0323),
        Instruction::JsrAbsolute(COLLECT_DIRECTORY_ADDRESS),
    ]);
    set_directory(&mut instructions, directories.dialogue);
    instructions.extend([
        Instruction::JsrAbsolute(project_dialogue_selector_address),
        Instruction::JsrAbsolute(COLLECT_DIRECTORY_ADDRESS),
        Instruction::JmpAbsolute(ALLOCATE_REMAP_PAIRS_ADDRESS),
    ]);
    assemble_at(DYNAMIC_ASSIGNMENT_CODE_CPU_ADDRESS, &instructions)
}

fn collect_recipe_colors() -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(RECIPE_POINTER_LOW),
        Instruction::StaZeroPage(RECIPE_PAIR_COUNT as u8),
    ];
    let done_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(COLLECT_RECIPE_ADDRESS));
    instructions.extend([
        Instruction::Clc,
        Instruction::LdaZeroPage(RECIPE_POINTER_LOW),
        Instruction::AdcImmediate(1),
        Instruction::StaZeroPage(RECIPE_POINTER_LOW),
        Instruction::LdaZeroPage(RECIPE_POINTER_HIGH),
        Instruction::AdcImmediate(0),
        Instruction::StaZeroPage(RECIPE_POINTER_HIGH),
    ]);
    let pair_loop = next_address(COLLECT_RECIPE_ADDRESS, &instructions)?;
    instructions.extend([
        Instruction::LdyImmediate(0),
        Instruction::LdaIndirectY(RECIPE_POINTER_LOW),
        Instruction::JsrAbsolute(MARK_SELECTED_COLOR_ADDRESS),
        Instruction::Clc,
        Instruction::LdaZeroPage(RECIPE_POINTER_LOW),
        Instruction::AdcImmediate(3),
        Instruction::StaZeroPage(RECIPE_POINTER_LOW),
        Instruction::LdaZeroPage(RECIPE_POINTER_HIGH),
        Instruction::AdcImmediate(0),
        Instruction::StaZeroPage(RECIPE_POINTER_HIGH),
        Instruction::DecAbsolute(RECIPE_PAIR_COUNT),
        Instruction::BneAbsolute(pair_loop),
    ]);
    let done = next_address(COLLECT_RECIPE_ADDRESS, &instructions)?;
    instructions.push(Instruction::Rts);
    instructions[done_placeholder] = Instruction::BeqAbsolute(done);
    assemble_at(COLLECT_RECIPE_ADDRESS, &instructions)
}

fn collect_directory_colors() -> Result<Vec<u8>> {
    assemble_at(
        COLLECT_DIRECTORY_ADDRESS,
        &[
            Instruction::AslAccumulator,
            Instruction::Tay,
            Instruction::LdaIndirectY(DIRECTORY_POINTER_LOW),
            Instruction::StaZeroPage(RECIPE_POINTER_LOW),
            Instruction::Iny,
            Instruction::LdaIndirectY(DIRECTORY_POINTER_LOW),
            Instruction::OraImmediate(0xB0),
            Instruction::StaZeroPage(RECIPE_POINTER_HIGH),
            Instruction::JmpAbsolute(COLLECT_RECIPE_ADDRESS),
        ],
    )
}

fn collect_participant_name_colors() -> Result<Vec<u8>> {
    assemble_at(
        COLLECT_PARTICIPANT_NAME_ADDRESS,
        &[
            Instruction::Sec,
            Instruction::SbcImmediate(1),
            Instruction::JmpAbsolute(COLLECT_DIRECTORY_ADDRESS),
        ],
    )
}

fn mark_selected_color() -> Result<Vec<u8>> {
    assemble_at(
        MARK_SELECTED_COLOR_ADDRESS,
        &[
            Instruction::Pha,
            Instruction::AndImmediate(7),
            Instruction::Tax,
            Instruction::LdaAbsoluteX(COLOR_BIT_MASKS_CPU_ADDRESS),
            Instruction::StaZeroPage(PHYSICAL_TILE_CODE),
            Instruction::Pla,
            Instruction::LsrAccumulator,
            Instruction::LsrAccumulator,
            Instruction::LsrAccumulator,
            Instruction::Tax,
            Instruction::LdaAbsoluteX(SELECTED_COLOR_BITMAP_ADDRESS),
            Instruction::OraZeroPage(PHYSICAL_TILE_CODE),
            Instruction::StaAbsoluteX(SELECTED_COLOR_BITMAP_ADDRESS),
            Instruction::Rts,
        ],
    )
}

fn allocate_remap_pairs(project_dialogue_selector_address: u16) -> Result<Vec<u8>> {
    let mut instructions = vec![
        Instruction::LdaImmediate(0),
        Instruction::StaZeroPage(ATLAS_POINTER_LOW),
        Instruction::StaZeroPage(ATLAS_POINTER_HIGH),
        Instruction::StaZeroPage(RECIPE_PAIR_COUNT as u8),
    ];
    let protected_loop = next_address(ALLOCATE_REMAP_PAIRS_ADDRESS, &instructions)?;
    instructions.extend([
        Instruction::LdxZeroPage(ATLAS_POINTER_LOW),
        Instruction::LdaAbsoluteX(PROTECTED_ABSTRACT_COLORS_CPU_ADDRESS),
        Instruction::StaZeroPage(DIRECTORY_POINTER_LOW),
        Instruction::JsrAbsolute(TEST_SELECTED_COLOR_ADDRESS),
    ]);
    let next_protected_placeholder = instructions.len();
    instructions.push(Instruction::BeqAbsolute(ALLOCATE_REMAP_PAIRS_ADDRESS));
    let find_safe = next_address(ALLOCATE_REMAP_PAIRS_ADDRESS, &instructions)?;
    instructions.extend([
        Instruction::LdxZeroPage(ATLAS_POINTER_HIGH),
        Instruction::CpxImmediate(SAFE_ABSTRACT_COLOR_COUNT as u8),
    ]);
    let exhausted_safe_placeholder = instructions.len();
    instructions.push(Instruction::BcsAbsolute(ALLOCATE_REMAP_PAIRS_ADDRESS));
    instructions.extend([
        Instruction::LdaAbsoluteX(SAFE_ABSTRACT_COLORS_CPU_ADDRESS),
        Instruction::StaZeroPage(DIRECTORY_POINTER_HIGH),
        Instruction::IncZeroPage(ATLAS_POINTER_HIGH),
        Instruction::JsrAbsolute(TEST_SELECTED_COLOR_ADDRESS),
        Instruction::BneAbsolute(find_safe),
        Instruction::LdaZeroPage(RECIPE_PAIR_COUNT as u8),
        Instruction::CmpImmediate(MAXIMUM_REMAP_PAIR_COUNT * 2),
    ]);
    let full_pair_table_placeholder = instructions.len();
    instructions.push(Instruction::BcsAbsolute(ALLOCATE_REMAP_PAIRS_ADDRESS));
    instructions.extend([
        Instruction::LdxZeroPage(DIRECTORY_POINTER_LOW),
        Instruction::LdaAbsoluteX(PHYSICAL_CODE_TABLE_CPU_ADDRESS),
        Instruction::StaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::LdxZeroPage(RECIPE_PAIR_COUNT as u8),
        Instruction::LdaZeroPage(PHYSICAL_TILE_CODE),
        Instruction::StaAbsoluteX(REMAP_PAIR_TABLE_ADDRESS),
        Instruction::LdxZeroPage(DIRECTORY_POINTER_HIGH),
        Instruction::LdaAbsoluteX(PHYSICAL_CODE_TABLE_CPU_ADDRESS),
        Instruction::LdxZeroPage(RECIPE_PAIR_COUNT as u8),
        Instruction::StaAbsoluteX(REMAP_PAIR_TABLE_ADDRESS + 1),
        Instruction::IncZeroPage(RECIPE_PAIR_COUNT as u8),
        Instruction::IncZeroPage(RECIPE_PAIR_COUNT as u8),
    ]);
    let next_protected = next_address(ALLOCATE_REMAP_PAIRS_ADDRESS, &instructions)?;
    instructions[next_protected_placeholder] = Instruction::BeqAbsolute(next_protected);
    instructions.extend([
        Instruction::IncZeroPage(ATLAS_POINTER_LOW),
        Instruction::LdaZeroPage(ATLAS_POINTER_LOW),
        Instruction::CmpImmediate(PROTECTED_ABSTRACT_COLOR_COUNT as u8),
        Instruction::BneAbsolute(protected_loop),
        Instruction::LdaZeroPage(RECIPE_PAIR_COUNT as u8),
        Instruction::StaAbsolute(REMAP_STATE_ADDRESS),
        // Publish the exact projected dialogue key only on success, after allocation has
        // finished, so an aborted composition cannot look cache-valid.
        Instruction::JsrAbsolute(project_dialogue_selector_address),
        Instruction::StaAbsolute(CACHED_DIALOGUE_SELECTOR_ADDRESS),
        Instruction::LdaImmediate(0),
        Instruction::Rts,
    ]);
    let overflow = next_address(ALLOCATE_REMAP_PAIRS_ADDRESS, &instructions)?;
    instructions.extend([Instruction::LdaImmediate(1), Instruction::Rts]);
    instructions[exhausted_safe_placeholder] = Instruction::BcsAbsolute(overflow);
    instructions[full_pair_table_placeholder] = Instruction::BcsAbsolute(overflow);
    assemble_at(ALLOCATE_REMAP_PAIRS_ADDRESS, &instructions)
}

fn test_selected_color() -> Result<Vec<u8>> {
    assemble_at(
        TEST_SELECTED_COLOR_ADDRESS,
        &[
            Instruction::Pha,
            Instruction::AndImmediate(7),
            Instruction::Tax,
            Instruction::LdaAbsoluteX(COLOR_BIT_MASKS_CPU_ADDRESS),
            Instruction::StaZeroPage(PHYSICAL_TILE_CODE),
            Instruction::Pla,
            Instruction::LsrAccumulator,
            Instruction::LsrAccumulator,
            Instruction::LsrAccumulator,
            Instruction::Tax,
            Instruction::LdaAbsoluteX(SELECTED_COLOR_BITMAP_ADDRESS),
            Instruction::AndZeroPage(PHYSICAL_TILE_CODE),
            Instruction::Rts,
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
        .context("battle dynamic-assignment routine address overflow")
}
