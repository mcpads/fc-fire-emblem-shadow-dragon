use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use serde_json::Value;

const MAPPER165_SOURCE_PAGE_BIAS: u8 = 2;
const MAPPER165_PAGE_REGISTER_SCALE: u8 = 4;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub(super) struct ChrPairReport {
    pub(super) left_fd: u8,
    pub(super) left_fe: u8,
    pub(super) right_fd: u8,
    pub(super) right_fe: u8,
}

pub(super) struct CaptureState {
    pub(super) producer_frame_count: u64,
    pub(super) chr_pair: ChrPairReport,
    pub(super) left_latch: u8,
    pub(super) right_latch: u8,
    pub(super) background_enabled: bool,
    pub(super) sprites_enabled: bool,
}

pub(super) fn parse_capture_state(bytes: &[u8]) -> Result<CaptureState> {
    let state: BTreeMap<String, Value> =
        serde_json::from_slice(bytes).context("parse producer state JSON")?;
    let unsigned = |key: &str| -> Result<u64> {
        state
            .get(key)
            .and_then(Value::as_u64)
            .with_context(|| format!("producer state has no unsigned {key}"))
    };
    let boolean = |key: &str| -> Result<bool> {
        state
            .get(key)
            .and_then(Value::as_bool)
            .with_context(|| format!("producer state has no boolean {key}"))
    };
    let byte = |key: &str| -> Result<u8> {
        unsigned(key)?
            .try_into()
            .with_context(|| format!("producer state {key} does not fit a byte"))
    };

    let (chr_pair, left_latch, right_latch) = if state.contains_key("mapper.leftChrPage[0]") {
        (
            ChrPairReport {
                left_fd: byte("mapper.leftChrPage[0]")?,
                left_fe: byte("mapper.leftChrPage[1]")?,
                right_fd: byte("mapper.rightChrPage[0]")?,
                right_fe: byte("mapper.rightChrPage[1]")?,
            },
            byte("mapper.leftLatch")?,
            byte("mapper.rightLatch")?,
        )
    } else {
        ensure!(
            byte("mapper.chrMode")? == 0,
            "mapper 165 capture is not using the required 4 KiB CHR mode"
        );
        (
            ChrPairReport {
                left_fd: decode_mapper165_page_register(byte("mapper.registers0")?)?,
                left_fe: decode_mapper165_page_register(byte("mapper.registers1")?)?,
                right_fd: decode_mapper165_page_register(byte("mapper.registers2")?)?,
                right_fe: decode_mapper165_page_register(byte("mapper.registers4")?)?,
            },
            u8::from(boolean("mapper.chrLatch[0]")?),
            u8::from(boolean("mapper.chrLatch[1]")?),
        )
    };

    Ok(CaptureState {
        producer_frame_count: unsigned("frameCount")?,
        chr_pair,
        left_latch,
        right_latch,
        background_enabled: boolean("ppu.mask.backgroundEnabled")?,
        sprites_enabled: boolean("ppu.mask.spritesEnabled")?,
    })
}

fn decode_mapper165_page_register(register: u8) -> Result<u8> {
    ensure!(
        register.is_multiple_of(MAPPER165_PAGE_REGISTER_SCALE),
        "mapper 165 CHR register 0x{register:02X} is not a 4 KiB ROM-page selection"
    );
    let physical_page = register / MAPPER165_PAGE_REGISTER_SCALE;
    physical_page
        .checked_sub(MAPPER165_SOURCE_PAGE_BIAS)
        .context(
            "mapper 165 CHR register selects the reserved prefix instead of source-relative ROM",
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapper165_registers_are_normalized_to_source_relative_pages() {
        assert_eq!(decode_mapper165_page_register(0x08).unwrap(), 0x00);
        assert_eq!(decode_mapper165_page_register(0x24).unwrap(), 0x07);
        assert_eq!(decode_mapper165_page_register(0x6C).unwrap(), 0x19);
    }

    #[test]
    fn mapper165_reserved_or_misaligned_registers_fail_closed() {
        assert!(decode_mapper165_page_register(0x01).is_err());
        assert!(decode_mapper165_page_register(0x04).is_err());
        assert!(decode_mapper165_page_register(0x09).is_err());
    }

    #[test]
    fn capture_parser_accepts_original_mmc4_state_shape() {
        let state = serde_json::json!({
            "frameCount": 123,
            "mapper.leftChrPage[0]": 2,
            "mapper.leftChrPage[1]": 6,
            "mapper.rightChrPage[0]": 0,
            "mapper.rightChrPage[1]": 25,
            "mapper.leftLatch": 0,
            "mapper.rightLatch": 1,
            "ppu.mask.backgroundEnabled": true,
            "ppu.mask.spritesEnabled": false
        });

        let parsed = parse_capture_state(&serde_json::to_vec(&state).unwrap()).unwrap();

        assert_eq!(parsed.producer_frame_count, 123);
        assert_eq!(
            parsed.chr_pair,
            ChrPairReport {
                left_fd: 2,
                left_fe: 6,
                right_fd: 0,
                right_fe: 25,
            }
        );
        assert_eq!((parsed.left_latch, parsed.right_latch), (0, 1));
        assert!(parsed.background_enabled);
        assert!(!parsed.sprites_enabled);
    }

    #[test]
    fn capture_parser_normalizes_mapper165_state_shape() {
        let state = serde_json::json!({
            "frameCount": 456,
            "mapper.registers0": 36,
            "mapper.registers1": 36,
            "mapper.registers2": 8,
            "mapper.registers4": 108,
            "mapper.chrMode": 0,
            "mapper.chrLatch[0]": true,
            "mapper.chrLatch[1]": false,
            "ppu.mask.backgroundEnabled": true,
            "ppu.mask.spritesEnabled": true
        });

        let parsed = parse_capture_state(&serde_json::to_vec(&state).unwrap()).unwrap();

        assert_eq!(parsed.producer_frame_count, 456);
        assert_eq!(
            parsed.chr_pair,
            ChrPairReport {
                left_fd: 7,
                left_fe: 7,
                right_fd: 0,
                right_fe: 25,
            }
        );
        assert_eq!((parsed.left_latch, parsed.right_latch), (1, 0));
        assert!(parsed.background_enabled);
        assert!(parsed.sprites_enabled);
    }
}
