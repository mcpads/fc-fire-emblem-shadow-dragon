use std::collections::BTreeSet;

use anyhow::{Result, ensure};

const CPU_MAPPING_PAGE_LEN: usize = 0x2000;

/// One instruction address in one explicitly selected 8 KiB physical PRG page.
///
/// `cpu_address` alone is insufficient: an old fixed page, the active expanded fixed page, and a
/// runtime-selected switchable page can all appear at the same CPU address.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct CodeLocation {
    pub(crate) region_role: String,
    pub(crate) physical_page_8k: u16,
    pub(crate) cpu_address: u16,
}

impl CodeLocation {
    pub(crate) fn new(
        region_role: impl Into<String>,
        physical_page_8k: u16,
        cpu_address: u16,
    ) -> Self {
        Self {
            region_role: region_role.into(),
            physical_page_8k,
            cpu_address,
        }
    }
}

/// Bytes belonging to one declared executable extent in one mapped 8 KiB physical page.
///
/// `bytes` may include trailing storage or cave padding. Only the prefix named by
/// `executable_len` can contain instruction starts or instruction bytes.
#[derive(Clone, Debug)]
pub(crate) struct ExecutableRegion<'a> {
    pub(crate) role: String,
    pub(crate) physical_page_8k: u16,
    pub(crate) cpu_start: u16,
    pub(crate) bytes: &'a [u8],
    pub(crate) executable_len: usize,
}

impl<'a> ExecutableRegion<'a> {
    pub(crate) fn new(
        role: impl Into<String>,
        physical_page_8k: u16,
        cpu_start: u16,
        bytes: &'a [u8],
        executable_len: usize,
    ) -> Result<Self> {
        let role = role.into();
        ensure!(!role.is_empty(), "executable-region role is empty");
        ensure!(
            executable_len > 0,
            "executable region {role} has an empty executable extent"
        );
        ensure!(
            executable_len <= bytes.len(),
            "executable region {role} declares {executable_len} executable bytes but contains only {} bytes",
            bytes.len()
        );
        let page_offset = usize::from(cpu_start & (CPU_MAPPING_PAGE_LEN as u16 - 1));
        ensure!(
            page_offset + bytes.len() <= CPU_MAPPING_PAGE_LEN,
            "executable region {role} crosses an 8 KiB CPU mapping boundary; split it into physical-page regions and bind the boundary explicitly"
        );
        Ok(Self {
            role,
            physical_page_8k,
            cpu_start,
            bytes,
            executable_len,
        })
    }

    pub(crate) fn location(&self, cpu_address: u16) -> CodeLocation {
        CodeLocation::new(self.role.clone(), self.physical_page_8k, cpu_address)
    }

    fn executable_offset(&self, location: &CodeLocation) -> Option<usize> {
        if location.region_role != self.role
            || location.physical_page_8k != self.physical_page_8k
            || location.cpu_address < self.cpu_start
        {
            return None;
        }
        let offset = usize::from(location.cpu_address - self.cpu_start);
        (offset < self.executable_len).then_some(offset)
    }

    fn executable_end_cpu_address(&self) -> u16 {
        self.cpu_start.wrapping_add(self.executable_len as u16)
    }
}

/// Explicit instruction-fetch continuation from one physical extent into another mapped page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SequentialCodeBoundary {
    pub(crate) after_region_role: String,
    pub(crate) to: CodeLocation,
}

impl SequentialCodeBoundary {
    pub(crate) fn new(after_region_role: impl Into<String>, to: CodeLocation) -> Self {
        Self {
            after_region_role: after_region_role.into(),
            to,
        }
    }
}

/// Explicit physical mapping for a direct branch, jump, or call leaving its current region.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DirectCodeBinding {
    pub(crate) from_region_role: String,
    pub(crate) target_cpu_address: u16,
    pub(crate) to: CodeLocation,
}

impl DirectCodeBinding {
    pub(crate) fn new(
        from_region_role: impl Into<String>,
        target_cpu_address: u16,
        to: CodeLocation,
    ) -> Self {
        Self {
            from_region_role: from_region_role.into(),
            target_cpu_address,
            to,
        }
    }
}

/// A caller-declared executable mapping and its rooted entry points.
///
/// This is deliberately a bounded program view, not a whole-ROM executable census. Cross-region
/// fetch and direct-control mappings must be supplied explicitly so a physical page is never
/// guessed from a CPU address. This type also does not execute mapper writes or mutate its own CPU
/// window mapping. A caller proving bank-changing code must provide the resulting bank-state
/// projections as separately bound mapping contexts.
#[derive(Clone, Debug)]
pub(crate) struct ExecutableProgram<'a> {
    pub(crate) role: String,
    pub(super) regions: Vec<ExecutableRegion<'a>>,
    pub(super) roots: Vec<CodeLocation>,
    sequential_boundaries: Vec<SequentialCodeBoundary>,
    direct_bindings: Vec<DirectCodeBinding>,
}

impl<'a> ExecutableProgram<'a> {
    pub(crate) fn new(
        role: impl Into<String>,
        regions: Vec<ExecutableRegion<'a>>,
        roots: Vec<CodeLocation>,
        sequential_boundaries: Vec<SequentialCodeBoundary>,
        direct_bindings: Vec<DirectCodeBinding>,
    ) -> Result<Self> {
        let role = role.into();
        ensure!(!role.is_empty(), "executable-program role is empty");
        ensure!(
            !regions.is_empty(),
            "executable program {role} has no regions"
        );
        ensure!(!roots.is_empty(), "executable program {role} has no roots");

        let mut region_roles = BTreeSet::new();
        for region in &regions {
            ensure!(
                region_roles.insert(region.role.as_str()),
                "executable program {role} repeats region role {}",
                region.role
            );
        }

        let program = Self {
            role,
            regions,
            roots,
            sequential_boundaries,
            direct_bindings,
        };
        program.validate_locations_and_bindings()?;
        Ok(program)
    }

    fn validate_locations_and_bindings(&self) -> Result<()> {
        for root in &self.roots {
            ensure!(
                self.region_for_location(root).is_some(),
                "executable program {} has an unmapped root {root:?}",
                self.role
            );
        }

        let mut sequential_sources = BTreeSet::new();
        for boundary in &self.sequential_boundaries {
            let source = self
                .region_by_role(&boundary.after_region_role)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "executable program {} binds an unknown sequential source role {}",
                        self.role,
                        boundary.after_region_role
                    )
                })?;
            ensure!(
                self.region_for_location(&boundary.to).is_some(),
                "executable program {} has an unmapped sequential target {:?}",
                self.role,
                boundary.to
            );
            ensure!(
                boundary.to.cpu_address == source.executable_end_cpu_address(),
                "executable program {} maps the byte after {} to ${:04X}, expected CPU ${:04X}",
                self.role,
                source.role,
                boundary.to.cpu_address,
                source.executable_end_cpu_address()
            );
            ensure!(
                sequential_sources.insert(boundary.after_region_role.as_str()),
                "executable program {} repeats the sequential boundary after {}",
                self.role,
                boundary.after_region_role
            );
        }

        let mut direct_sources = BTreeSet::new();
        for binding in &self.direct_bindings {
            ensure!(
                self.region_by_role(&binding.from_region_role).is_some(),
                "executable program {} binds a direct edge from unknown region {}",
                self.role,
                binding.from_region_role
            );
            ensure!(
                self.region_for_location(&binding.to).is_some(),
                "executable program {} has an unmapped direct target {:?}",
                self.role,
                binding.to
            );
            ensure!(
                binding.target_cpu_address == binding.to.cpu_address,
                "executable program {} maps direct CPU target ${:04X} to mismatched CPU address ${:04X}",
                self.role,
                binding.target_cpu_address,
                binding.to.cpu_address
            );
            ensure!(
                direct_sources.insert((
                    binding.from_region_role.as_str(),
                    binding.target_cpu_address,
                )),
                "executable program {} repeats a direct binding from {} to ${:04X}",
                self.role,
                binding.from_region_role,
                binding.target_cpu_address
            );
        }
        Ok(())
    }

    fn region_by_role(&self, role: &str) -> Option<&ExecutableRegion<'a>> {
        self.regions.iter().find(|region| region.role == role)
    }

    fn region_for_location(&self, location: &CodeLocation) -> Option<&ExecutableRegion<'a>> {
        self.region_by_role(&location.region_role)
            .filter(|region| region.executable_offset(location).is_some())
    }

    pub(super) fn byte_at(&self, location: &CodeLocation) -> Option<u8> {
        let region = self.region_for_location(location)?;
        region
            .bytes
            .get(region.executable_offset(location)?)
            .copied()
    }

    pub(super) fn sequential_location_after(
        &self,
        location: &CodeLocation,
    ) -> Option<CodeLocation> {
        let region = self.region_for_location(location)?;
        let offset = region.executable_offset(location)?;
        if offset + 1 < region.executable_len {
            return Some(region.location(location.cpu_address.wrapping_add(1)));
        }
        self.sequential_boundaries
            .iter()
            .find(|boundary| boundary.after_region_role == region.role)
            .map(|boundary| boundary.to.clone())
    }

    pub(super) fn resolve_direct_target(
        &self,
        from: &CodeLocation,
        target_cpu_address: u16,
    ) -> Option<CodeLocation> {
        if let Some(binding) = self.direct_bindings.iter().find(|binding| {
            binding.from_region_role == from.region_role
                && binding.target_cpu_address == target_cpu_address
        }) {
            return Some(binding.to.clone());
        }
        let source_region = self.region_for_location(from)?;
        let target = source_region.location(target_cpu_address);
        source_region
            .executable_offset(&target)
            .is_some()
            .then_some(target)
    }
}
