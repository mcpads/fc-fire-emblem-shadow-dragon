/// CPU-register classes decoded by the source MMC4.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum SourceMmc4Register {
    PrgBank,
    LeftFdChrBank,
    LeftFeChrBank,
    RightFdChrBank,
    RightFeChrBank,
    Mirroring,
}

/// Decode every source-MMC4 write alias, not only the canonical page-start addresses.
pub(crate) const fn decode_source_mmc4_write(address: u16) -> Option<SourceMmc4Register> {
    match address >> 12 {
        0xA => Some(SourceMmc4Register::PrgBank),
        0xB => Some(SourceMmc4Register::LeftFdChrBank),
        0xC => Some(SourceMmc4Register::LeftFeChrBank),
        0xD => Some(SourceMmc4Register::RightFdChrBank),
        0xE => Some(SourceMmc4Register::RightFeChrBank),
        0xF => Some(SourceMmc4Register::Mirroring),
        _ => None,
    }
}

/// CPU-register classes decoded by mapper 165's MMC3-compatible register interface.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum Mapper165Register {
    BankSelect,
    BankData,
    Mirroring,
    PrgRamProtect,
    IrqLatch,
    IrqReload,
    IrqDisable,
    IrqEnable,
}

/// Decode every mapper-165/MMC3 write alias using the hardware `$E001` address mask.
pub(crate) const fn decode_mapper165_write(address: u16) -> Option<Mapper165Register> {
    if address < 0x8000 {
        return None;
    }
    match address & 0xE001 {
        0x8000 => Some(Mapper165Register::BankSelect),
        0x8001 => Some(Mapper165Register::BankData),
        0xA000 => Some(Mapper165Register::Mirroring),
        0xA001 => Some(Mapper165Register::PrgRamProtect),
        0xC000 => Some(Mapper165Register::IrqLatch),
        0xC001 => Some(Mapper165Register::IrqReload),
        0xE000 => Some(Mapper165Register::IrqDisable),
        0xE001 => Some(Mapper165Register::IrqEnable),
        _ => None,
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MapperHardware {
    SourceMmc4,
    Mapper165,
}

/// Hardware register selected by one statically direct write.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum MapperRegister {
    SourceMmc4(SourceMmc4Register),
    Mapper165(Mapper165Register),
}

#[cfg(test)]
impl MapperHardware {
    pub(super) const fn decode_write(self, address: u16) -> Option<MapperRegister> {
        match self {
            Self::SourceMmc4 => match decode_source_mmc4_write(address) {
                Some(register) => Some(MapperRegister::SourceMmc4(register)),
                None => None,
            },
            Self::Mapper165 => match decode_mapper165_write(address) {
                Some(register) => Some(MapperRegister::Mapper165(register)),
                None => None,
            },
        }
    }
}
