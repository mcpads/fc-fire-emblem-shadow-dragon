use super::*;

pub(super) fn pack_logical_records(
    records: &[&LogicalDialogueRecord],
) -> (Vec<LogicalDialogueByte>, Vec<usize>) {
    let record_bytes = records
        .iter()
        .map(|record| record.bytes.as_slice())
        .collect::<Vec<_>>();
    pack_record_bytes(&record_bytes)
}

pub(super) fn pack_record_bytes<T: Clone + PartialEq>(records: &[&[T]]) -> (Vec<T>, Vec<usize>) {
    let mut storage = Vec::new();
    let mut placements = Vec::with_capacity(records.len());
    for record in records {
        if let Some(existing_offset) = find_subsequence(&storage, record) {
            placements.push(existing_offset);
            continue;
        }
        let overlap = (1..=storage.len().min(record.len()))
            .rev()
            .find(|overlap| storage[storage.len() - overlap..] == record[..*overlap])
            .unwrap_or(0);
        placements.push(storage.len() - overlap);
        storage.extend_from_slice(&record[overlap..]);
    }
    (storage, placements)
}

fn find_subsequence<T: PartialEq>(storage: &[T], record: &[T]) -> Option<usize> {
    (!record.is_empty())
        .then(|| {
            storage
                .windows(record.len())
                .position(|window| window == record)
        })
        .flatten()
}
