use anyhow::{Context, Result, ensure};

use crate::{
    rom::Rom,
    rp2a03::{Instruction, assemble_at},
    source_prg::switchable_bank_file_offset,
    tracked::TrackedImage,
};

const HP_BAR_QUEUE_PRODUCER_BANK: u8 = 0x05;
const HP_BAR_QUEUE_LENGTH_PUBLICATION_ADDRESS: u16 = 0x8B6D;

pub(super) fn bind_hp_bar_queue_publication_source(source: &Rom) -> Result<()> {
    let expected = source_hp_bar_queue_length_derivation()?;
    let offset = hp_bar_queue_length_publication_file_offset()?;
    let actual = source
        .data()
        .get(offset..offset + expected.len())
        .context("HP-bar queue length derivation is outside the source image")?;
    ensure!(
        actual == expected,
        "HP-bar queue length derivation changed in the source image"
    );
    Ok(())
}

pub(super) fn install_emitted_hp_bar_payload_length(image: &mut TrackedImage) -> Result<()> {
    image.write_expected(
        "publish the emitted HP-bar payload length",
        hp_bar_queue_length_publication_file_offset()?,
        &source_hp_bar_queue_length_derivation()?,
        &emitted_hp_bar_payload_length_publication()?,
    )
}

pub(crate) fn emitted_hp_bar_payload_length_publication() -> Result<Vec<u8>> {
    let mut instructions = vec![Instruction::Txa];
    instructions.extend(std::iter::repeat_n(Instruction::Nop, 7));
    assemble_at(HP_BAR_QUEUE_LENGTH_PUBLICATION_ADDRESS, &instructions)
}

pub(crate) fn hp_bar_queue_length_publication_file_offset() -> Result<usize> {
    switchable_bank_file_offset(
        HP_BAR_QUEUE_PRODUCER_BANK,
        HP_BAR_QUEUE_LENGTH_PUBLICATION_ADDRESS,
    )
}

fn source_hp_bar_queue_length_derivation() -> Result<Vec<u8>> {
    assemble_at(
        HP_BAR_QUEUE_LENGTH_PUBLICATION_ADDRESS,
        &[
            Instruction::LdaZeroPage(0x03),
            Instruction::LsrAccumulator,
            Instruction::Tax,
            Instruction::BccAbsolute(0x8B74),
            Instruction::Inx,
            Instruction::Txa,
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST_ROW_PPU_ADDRESS: u16 = 0x22E2;
    const SECOND_ROW_PPU_ADDRESS: u16 = FIRST_ROW_PPU_ADDRESS - 0x20;
    const HP_BAR_TILES_PER_ROW: usize = 13;
    const MAXIMUM_HP_BAR_PAYLOAD_BYTE_COUNT: usize = HP_BAR_TILES_PER_ROW * 2;

    #[test]
    fn publication_loads_the_emitted_payload_count_from_x() {
        let bytes = emitted_hp_bar_payload_length_publication().unwrap();

        assert_eq!(
            bytes.len(),
            source_hp_bar_queue_length_derivation().unwrap().len()
        );
        assert_eq!(
            bytes,
            assemble_at(
                HP_BAR_QUEUE_LENGTH_PUBLICATION_ADDRESS,
                &[
                    Instruction::Txa,
                    Instruction::Nop,
                    Instruction::Nop,
                    Instruction::Nop,
                    Instruction::Nop,
                    Instruction::Nop,
                    Instruction::Nop,
                    Instruction::Nop,
                ]
            )
            .unwrap()
        );
    }

    #[test]
    fn every_supported_payload_terminates_after_one_or_two_nonempty_commands() {
        for payload_byte_count in 1..=MAXIMUM_HP_BAR_PAYLOAD_BYTE_COUNT {
            let payload = (0..payload_byte_count)
                .map(|index| 0xC0 + u8::try_from(index).unwrap())
                .collect::<Vec<_>>();
            let queue = publish_queue(&payload, payload.len());
            let commands = parse_queue(&queue).unwrap();

            assert_eq!(
                commands
                    .iter()
                    .map(|command| command.payload.len())
                    .sum::<usize>(),
                payload.len()
            );
            assert!(commands.iter().all(|command| !command.payload.is_empty()));
            assert!(
                commands
                    .iter()
                    .all(|command| command.payload.len() <= HP_BAR_TILES_PER_ROW)
            );
            assert_eq!(commands[0].ppu_address, FIRST_ROW_PPU_ADDRESS);
            if commands.len() == 2 {
                assert_eq!(commands[1].ppu_address, SECOND_ROW_PPU_ADDRESS);
            }
        }
    }

    #[test]
    fn reproduced_eight_byte_payload_cannot_be_published_as_five_bytes() {
        let payload = vec![0xCE; 8];
        let stale_length_queue = publish_queue(&payload, 5);
        let emitted_length_queue = publish_queue(&payload, payload.len());

        assert!(parse_queue(&stale_length_queue).is_err());
        assert_eq!(
            parse_queue(&emitted_length_queue)
                .unwrap()
                .into_iter()
                .flat_map(|command| command.payload)
                .collect::<Vec<_>>(),
            payload
        );
    }

    #[derive(Debug)]
    struct ParsedCommand {
        ppu_address: u16,
        payload: Vec<u8>,
    }

    fn publish_queue(payload: &[u8], published_payload_byte_count: usize) -> Vec<u8> {
        assert!(!payload.is_empty());
        assert!(payload.len() <= MAXIMUM_HP_BAR_PAYLOAD_BYTE_COUNT);
        assert!(published_payload_byte_count <= payload.len());

        let first_length = published_payload_byte_count.min(HP_BAR_TILES_PER_ROW);
        let second_length = published_payload_byte_count.saturating_sub(HP_BAR_TILES_PER_ROW);
        let mut queue = command_header(FIRST_ROW_PPU_ADDRESS, first_length);
        queue.extend_from_slice(&payload[..first_length]);
        if second_length > 0 {
            queue.extend(command_header(SECOND_ROW_PPU_ADDRESS, second_length));
            queue.extend_from_slice(&payload[first_length..first_length + second_length]);
        }
        queue.extend_from_slice(&payload[published_payload_byte_count..]);
        queue.push(0);
        queue
    }

    fn command_header(ppu_address: u16, published_length: usize) -> Vec<u8> {
        let [high, low] = ppu_address.to_be_bytes();
        vec![high, low, u8::try_from(published_length).unwrap()]
    }

    fn parse_queue(queue: &[u8]) -> std::result::Result<Vec<ParsedCommand>, &'static str> {
        let mut cursor = 0;
        let mut commands = Vec::new();
        loop {
            let high = *queue.get(cursor).ok_or("queue has no terminator")?;
            cursor += 1;
            if high == 0 {
                return (cursor == queue.len())
                    .then_some(commands)
                    .ok_or("queue has bytes after its terminator");
            }
            let low = *queue.get(cursor).ok_or("queue address is truncated")?;
            let published_length =
                usize::from(*queue.get(cursor + 1).ok_or("queue length is missing")?);
            cursor += 2;
            if published_length == 0 {
                return Err("zero-length PPU command would underflow to 256 writes");
            }
            let payload = queue
                .get(cursor..cursor + published_length)
                .ok_or("queue payload is shorter than its published length")?
                .to_vec();
            cursor += published_length;
            commands.push(ParsedCommand {
                ppu_address: u16::from_be_bytes([high, low]),
                payload,
            });
        }
    }
}
