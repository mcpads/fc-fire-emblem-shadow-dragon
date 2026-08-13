use std::{fs, path::Path};

use anyhow::{Context, Result, ensure};

use crate::sha1_hex;

pub const HEADER_SIZE: usize = 16;
pub const PRG_SIZE: usize = 256 * 1024;
pub const CHR_SIZE: usize = 128 * 1024;
pub const CHR_FILE_OFFSET: usize = HEADER_SIZE + PRG_SIZE;

pub const EXPECTED_SOURCE_SHA1: &str = "0179c550d424e0397496078789e7b116601d120c";
pub const EXPECTED_PRG_SHA1: &str = "a74c0760f32f5131d4feb67694123fda6b7da24f";
pub const EXPECTED_CHR_SHA1: &str = "98ee1568073c6c41425e4a381cf588433ac9fc97";
pub const EXPECTED_HEADER: [u8; HEADER_SIZE] = [
    0x4E, 0x45, 0x53, 0x1A, 0x10, 0x10, 0xA2, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

#[derive(Debug, Clone)]
pub struct Rom {
    data: Vec<u8>,
    header: [u8; HEADER_SIZE],
    prg_range: std::ops::Range<usize>,
    chr_range: std::ops::Range<usize>,
    mapper: u16,
}

impl Rom {
    pub fn from_path(path: &Path) -> Result<Self> {
        let data = fs::read(path).with_context(|| format!("read ROM {}", path.display()))?;
        Self::parse(data).with_context(|| format!("parse ROM {}", path.display()))
    }

    pub fn parse(data: Vec<u8>) -> Result<Self> {
        ensure!(
            data.len() >= HEADER_SIZE && &data[..4] == b"NES\x1A",
            "not an iNES image"
        );
        let header: [u8; HEADER_SIZE] = data[..HEADER_SIZE].try_into().unwrap();
        ensure!(
            header[6] & 0x04 == 0,
            "trainer-bearing images are unsupported"
        );
        // 배포 이미지는 NES 2.0이다. iNES 1.0에는 CHR ROM과 별개인 CHR RAM을 적을
        // 자리가 없기 때문이다. 의사결정 62번을 따른다. 크기 상위 니블은 바이트 9에
        // 있고, 니블이 `F`면 지수 표기인데 이 프로젝트의 크기는 모두 선형 표기 안에
        // 들어가므로 지수 표기는 받지 않는다.
        let nes20 = header[7] & 0x0C == 0x08;
        let (prg_banks, chr_banks) = if nes20 {
            ensure!(
                header[9] & 0x0F != 0x0F && header[9] >> 4 != 0x0F,
                "NES 2.0 exponent size form is unsupported"
            );
            (
                usize::from(header[4]) | (usize::from(header[9] & 0x0F) << 8),
                usize::from(header[5]) | (usize::from(header[9] >> 4) << 8),
            )
        } else {
            (usize::from(header[4]), usize::from(header[5]))
        };

        let prg_size = prg_banks * 16 * 1024;
        let chr_size = chr_banks * 8 * 1024;
        let payload_end = HEADER_SIZE + prg_size + chr_size;
        ensure!(
            data.len() == payload_end,
            "header requires {payload_end} bytes, found {}",
            data.len()
        );
        let mapper = ((header[6] >> 4) as u16) | ((header[7] & 0xF0) as u16);

        Ok(Self {
            data,
            header,
            prg_range: HEADER_SIZE..HEADER_SIZE + prg_size,
            chr_range: HEADER_SIZE + prg_size..payload_end,
            mapper,
        })
    }

    pub fn verify_supported_japanese(&self) -> Result<()> {
        let actual_sha1 = sha1_hex(&self.data);
        ensure!(
            actual_sha1 == EXPECTED_SOURCE_SHA1,
            "source SHA-1 mismatch: expected {EXPECTED_SOURCE_SHA1}, found {actual_sha1}"
        );
        ensure!(
            self.header == EXPECTED_HEADER,
            "source header mismatch: expected {}, found {}",
            hex(&EXPECTED_HEADER),
            hex(&self.header)
        );
        ensure!(
            self.mapper == 10,
            "mapper mismatch: expected 10, found {}",
            self.mapper
        );
        ensure!(self.prg().len() == PRG_SIZE, "unexpected source PRG size");
        ensure!(self.chr().len() == CHR_SIZE, "unexpected source CHR size");
        ensure!(
            sha1_hex(self.prg()) == EXPECTED_PRG_SHA1,
            "source PRG SHA-1 mismatch"
        );
        ensure!(
            sha1_hex(self.chr()) == EXPECTED_CHR_SHA1,
            "source CHR SHA-1 mismatch"
        );
        Ok(())
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn prg(&self) -> &[u8] {
        &self.data[self.prg_range.clone()]
    }

    pub fn chr(&self) -> &[u8] {
        &self.data[self.chr_range.clone()]
    }

    pub fn mapper(&self) -> u16 {
        self.mapper
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mapper_10_ines_layout() {
        let mut image = vec![0; HEADER_SIZE + PRG_SIZE + CHR_SIZE];
        image[..HEADER_SIZE].copy_from_slice(&EXPECTED_HEADER);
        let rom = Rom::parse(image).unwrap();

        assert_eq!(rom.mapper(), 10);
        assert_eq!(rom.prg().len(), PRG_SIZE);
        assert_eq!(rom.chr().len(), CHR_SIZE);
    }

    #[test]
    fn rejects_a_trainer_bearing_image() {
        let mut image = vec![0; HEADER_SIZE + PRG_SIZE + CHR_SIZE];
        image[..HEADER_SIZE].copy_from_slice(&EXPECTED_HEADER);
        image[6] |= 0x04;

        assert!(Rom::parse(image).is_err());
    }
}
