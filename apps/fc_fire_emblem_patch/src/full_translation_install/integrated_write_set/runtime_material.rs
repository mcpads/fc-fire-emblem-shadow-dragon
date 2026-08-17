use anyhow::{Result, ensure};

use crate::rom::Rom;

use super::{
    super::{
        installation_layout::main_dialogue_runtime_material_file_offset,
        runtime_code::DialogueRuntimeCodePlan,
    },
    technical_installation::{
        IntegratedImage, mutation_expected_slice, runtime_material_routine_file_offset,
        verify_runtime_material_code_projection,
    },
};

pub(super) const RUNTIME_MATERIAL_DATA_ROLE: &str = "main dialogue runtime material data";

pub(super) fn install_dialogue_runtime_material(
    image: &mut IntegratedImage,
    candidate: &Rom,
    material: &[u8],
    runtime_code: &DialogueRuntimeCodePlan,
) -> Result<()> {
    let material_offset = main_dialogue_runtime_material_file_offset()?;
    let code_page_offset = verify_runtime_material_code_projection(material, runtime_code)?;
    let whole_expected = mutation_expected_slice(
        candidate.data(),
        material_offset,
        material.len(),
        "main dialogue runtime material",
    )?;
    ensure!(
        whole_expected.iter().all(|byte| *byte == 0xFF),
        "dialogue runtime material destination is not exact FF"
    );
    image.write_expected(
        RUNTIME_MATERIAL_DATA_ROLE,
        material_offset,
        &whole_expected[..code_page_offset],
        &material[..code_page_offset],
    )?;
    for routine in &runtime_code.code_routines {
        let offset = runtime_material_routine_file_offset(
            material_offset,
            code_page_offset,
            routine.address,
            routine.bytes.len(),
        )?;
        let expected =
            mutation_expected_slice(candidate.data(), offset, routine.bytes.len(), routine.role)?;
        image.write_runtime_routine(
            routine.role,
            offset,
            expected,
            &routine.bytes,
            routine.address,
        )?;
    }
    Ok(())
}
