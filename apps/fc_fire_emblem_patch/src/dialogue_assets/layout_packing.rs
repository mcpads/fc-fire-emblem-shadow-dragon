use super::*;

pub(super) fn pack_logical_records(
    records: &[&LogicalDialogueRecord],
) -> (Vec<LogicalDialogueByte>, Vec<usize>) {
    let mut storage = Vec::new();
    let mut placements = Vec::with_capacity(records.len());
    for record in records {
        if let Some(existing_offset) = find_subsequence(&storage, &record.bytes) {
            placements.push(existing_offset);
            continue;
        }
        let overlap = (1..=storage.len().min(record.bytes.len()))
            .rev()
            .find(|overlap| storage[storage.len() - overlap..] == record.bytes[..*overlap])
            .unwrap_or(0);
        placements.push(storage.len() - overlap);
        storage.extend_from_slice(&record.bytes[overlap..]);
    }
    (storage, placements)
}

pub(super) fn find_subsequence(
    storage: &[LogicalDialogueByte],
    record: &[LogicalDialogueByte],
) -> Option<usize> {
    (!record.is_empty())
        .then(|| {
            storage
                .windows(record.len())
                .position(|window| window == record)
        })
        .flatten()
}
