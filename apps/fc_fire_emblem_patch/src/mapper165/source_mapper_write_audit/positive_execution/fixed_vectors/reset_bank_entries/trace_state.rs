use std::ops::RangeInclusive;

#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum ByteValueSet {
    #[default]
    Unknown,
    Known(ByteDomain),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct ByteDomain([u64; 4]);

impl ByteDomain {
    fn singleton(value: u8) -> Self {
        let mut words = [0_u64; 4];
        words[usize::from(value) / 64] |= 1_u64 << (value % 64);
        Self(words)
    }

    fn values(self) -> impl Iterator<Item = u8> {
        (u8::MIN..=u8::MAX).filter(move |value| self.contains(*value))
    }

    fn contains(self, value: u8) -> bool {
        self.0[usize::from(value) / 64] & (1_u64 << (value % 64)) != 0
    }

    fn union(self, other: Self) -> Self {
        Self(std::array::from_fn(|index| self.0[index] | other.0[index]))
    }

    fn is_empty(self) -> bool {
        self.0 == [0; 4]
    }
}

impl ByteValueSet {
    pub(super) fn known(value: u8) -> Self {
        Self::Known(ByteDomain::singleton(value))
    }

    fn from_optional(value: Option<u8>) -> Self {
        match value {
            Some(value) => Self::known(value),
            None => Self::Unknown,
        }
    }

    pub(super) fn known_values(&self) -> Option<Vec<u8>> {
        match self {
            Self::Unknown => None,
            Self::Known(values) => Some(values.values().collect()),
        }
    }

    pub(super) fn singleton(&self) -> Option<u8> {
        let values = self.known_values()?;
        (values.len() == 1).then(|| values[0])
    }

    pub(super) fn map(&self, transform: impl Fn(u8) -> u8) -> Self {
        match self {
            Self::Unknown => Self::Unknown,
            Self::Known(values) => {
                let mut mapped = [0_u64; 4];
                for value in values.values().map(transform) {
                    mapped[usize::from(value) / 64] |= 1_u64 << (value % 64);
                }
                Self::Known(ByteDomain(mapped))
            }
        }
    }

    pub(super) fn uniform(&self, predicate: impl Fn(u8) -> bool) -> Option<bool> {
        let values = self.known_values()?;
        let mut results = values.into_iter().map(predicate);
        let first = results.next()?;
        results.all(|result| result == first).then_some(first)
    }

    pub(super) fn restrict(&self, predicate: impl Fn(u8) -> bool) -> Option<Self> {
        let mut restricted = [0_u64; 4];
        for value in u8::MIN..=u8::MAX {
            if matches!(self, Self::Known(values) if !values.contains(value)) || !predicate(value) {
                continue;
            }
            restricted[usize::from(value) / 64] |= 1_u64 << (value % 64);
        }
        let restricted = ByteDomain(restricted);
        (!restricted.is_empty()).then_some(Self::Known(restricted))
    }

    fn union(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Unknown, _) | (_, Self::Unknown) => Self::Unknown,
            (Self::Known(left), Self::Known(right)) => Self::Known(left.union(*right)),
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum ReturnFrame {
    Direct(u16),
    Banked {
        continuation: Box<ReturnFrame>,
        restore_bank: Option<u8>,
    },
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct ActivationId(pub(super) usize);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct ReturnContinuation {
    pub(super) parent: ActivationId,
    pub(super) frame: ReturnFrame,
}

#[derive(Clone, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
struct ResetTraceMemory {
    pointer_low_00: ByteValueSet,
    pointer_high_01: ByteValueSet,
    saved_prg_bank_08: ByteValueSet,
    pointer_high_09: ByteValueSet,
    title_animation_staging_0a: ByteValueSet,
    outer_screen_state_24: ByteValueSet,
    scheduler_state_25: ByteValueSet,
    primary_prg_bank_shadow_29: ByteValueSet,
    far_call_selector_44: ByteValueSet,
    restored_prg_bank_shadow_51: ByteValueSet,
    main_state_84: ByteValueSet,
    victory_stage_053e: ByteValueSet,
    title_state_057a: ByteValueSet,
    title_animation_state_0587: ByteValueSet,
    temporary_sprite_prg_bank_05c6: ByteValueSet,
    pending_state_request_05cc: ByteValueSet,
    requested_prg_bank_05c7: ByteValueSet,
    outer_state_05db: ByteValueSet,
    menu_state_05de: ByteValueSet,
    composite_state_05e8: ByteValueSet,
    dialogue_or_sound_state_05ee: ByteValueSet,
}

impl ResetTraceMemory {
    const ADDRESSES: [u16; 21] = [
        0x0000, 0x0001, 0x0008, 0x0009, 0x000A, 0x0024, 0x0025, 0x0029, 0x0044, 0x0051, 0x0084,
        0x053E, 0x057A, 0x0587, 0x05C6, 0x05C7, 0x05CC, 0x05DB, 0x05DE, 0x05E8, 0x05EE,
    ];

    fn read(&self, address: u16) -> ByteValueSet {
        match address {
            0x0000 => self.pointer_low_00.clone(),
            0x0001 => self.pointer_high_01.clone(),
            0x0008 => self.saved_prg_bank_08.clone(),
            0x0009 => self.pointer_high_09.clone(),
            0x000A => self.title_animation_staging_0a.clone(),
            0x0024 => self.outer_screen_state_24.clone(),
            0x0025 => self.scheduler_state_25.clone(),
            0x0029 => self.primary_prg_bank_shadow_29.clone(),
            0x0044 => self.far_call_selector_44.clone(),
            0x0051 => self.restored_prg_bank_shadow_51.clone(),
            0x0084 => self.main_state_84.clone(),
            0x053E => self.victory_stage_053e.clone(),
            0x057A => self.title_state_057a.clone(),
            0x0587 => self.title_animation_state_0587.clone(),
            0x05C6 => self.temporary_sprite_prg_bank_05c6.clone(),
            0x05C7 => self.requested_prg_bank_05c7.clone(),
            0x05CC => self.pending_state_request_05cc.clone(),
            0x05DB => self.outer_state_05db.clone(),
            0x05DE => self.menu_state_05de.clone(),
            0x05E8 => self.composite_state_05e8.clone(),
            0x05EE => self.dialogue_or_sound_state_05ee.clone(),
            _ => ByteValueSet::Unknown,
        }
    }

    fn write(&mut self, address: u16, value: ByteValueSet) {
        match address {
            0x0000 => self.pointer_low_00 = value,
            0x0001 => self.pointer_high_01 = value,
            0x0008 => self.saved_prg_bank_08 = value,
            0x0009 => self.pointer_high_09 = value,
            0x000A => self.title_animation_staging_0a = value,
            0x0024 => self.outer_screen_state_24 = value,
            0x0025 => self.scheduler_state_25 = value,
            0x0029 => self.primary_prg_bank_shadow_29 = value,
            0x0044 => self.far_call_selector_44 = value,
            0x0051 => self.restored_prg_bank_shadow_51 = value,
            0x0084 => self.main_state_84 = value,
            0x053E => self.victory_stage_053e = value,
            0x057A => self.title_state_057a = value,
            0x0587 => self.title_animation_state_0587 = value,
            0x05C6 => self.temporary_sprite_prg_bank_05c6 = value,
            0x05C7 => self.requested_prg_bank_05c7 = value,
            0x05CC => self.pending_state_request_05cc = value,
            0x05DB => self.outer_state_05db = value,
            0x05DE => self.menu_state_05de = value,
            0x05E8 => self.composite_state_05e8 = value,
            0x05EE => self.dialogue_or_sound_state_05ee = value,
            _ => {}
        }
    }

    fn clear_all(&mut self) {
        *self = Self::default();
    }

    fn clear_intersecting(&mut self, destination_ranges: &[RangeInclusive<u16>]) {
        for address in Self::ADDRESSES {
            if destination_ranges
                .iter()
                .any(|range| range.contains(&address))
            {
                self.write(address, ByteValueSet::Unknown);
            }
        }
    }

    fn initialize_after_ram_clear(&mut self) {
        for address in Self::ADDRESSES {
            self.write(address, ByteValueSet::from_optional(Some(0)));
        }
        self.pointer_high_01 = ByteValueSet::from_optional(Some(0xFF));
    }

    fn union(&self, other: &Self) -> Self {
        let mut joined = Self::default();
        for address in Self::ADDRESSES {
            joined.write(address, self.read(address).union(&other.read(address)));
        }
        joined
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct ResetTraceState {
    pub(super) address: u16,
    pub(super) accumulator: ByteValueSet,
    pub(super) index_x: Option<u8>,
    pub(super) index_y: Option<u8>,
    pub(super) zero: Option<bool>,
    pub(super) negative: Option<bool>,
    pub(super) carry: Option<bool>,
    memory: ResetTraceMemory,
    pub(super) mapped_prg_bank: Option<u8>,
    pub(super) activation: ActivationId,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct ResetTraceIdentity {
    address: u16,
    mapped_prg_bank: Option<u8>,
    activation: ActivationId,
}

impl ResetTraceState {
    pub(super) fn tracks_memory_address(address: u16) -> bool {
        ResetTraceMemory::ADDRESSES.contains(&address)
    }

    pub(super) fn at(address: u16, activation: ActivationId) -> Self {
        Self {
            address,
            accumulator: ByteValueSet::Unknown,
            index_x: None,
            index_y: None,
            zero: None,
            negative: None,
            carry: None,
            memory: ResetTraceMemory::default(),
            mapped_prg_bank: None,
            activation,
        }
    }

    pub(super) fn set_accumulator(&mut self, value: Option<u8>) {
        self.set_accumulator_values(ByteValueSet::from_optional(value));
    }

    pub(super) fn set_accumulator_values(&mut self, values: ByteValueSet) {
        self.zero = values.uniform(|value| value == 0);
        self.negative = values.uniform(|value| value & 0x80 != 0);
        self.accumulator = values;
    }

    pub(super) fn set_index_x(&mut self, value: Option<u8>) {
        self.set_zero_negative(value);
        self.index_x = value;
    }

    pub(super) fn set_index_y(&mut self, value: Option<u8>) {
        self.set_zero_negative(value);
        self.index_y = value;
    }

    pub(super) fn set_zero_negative(&mut self, value: Option<u8>) {
        self.zero = value.map(|value| value == 0);
        self.negative = value.map(|value| value & 0x80 != 0);
    }

    pub(super) fn invalidate_registers_and_flags(&mut self) {
        self.accumulator = ByteValueSet::Unknown;
        self.index_x = None;
        self.index_y = None;
        self.zero = None;
        self.negative = None;
        self.carry = None;
    }

    pub(super) fn read_memory(&self, address: u16) -> Option<u8> {
        self.memory.read(address).singleton()
    }

    pub(super) fn read_memory_values(&self, address: u16) -> ByteValueSet {
        self.memory.read(address)
    }

    pub(super) fn write_memory(&mut self, address: u16, value: Option<u8>) {
        self.memory
            .write(address, ByteValueSet::from_optional(value));
    }

    pub(super) fn write_memory_values(&mut self, address: u16, values: ByteValueSet) {
        self.memory.write(address, values);
    }

    pub(super) fn write_prg_bank_shadows(&mut self, value: Option<u8>) {
        let values = ByteValueSet::from_optional(value);
        self.memory.write(0x0029, values.clone());
        self.memory.write(0x0051, values);
    }

    pub(super) fn clear_memory_and_bank(&mut self) {
        self.memory.clear_all();
        self.mapped_prg_bank = None;
    }

    pub(super) fn clear_memory_in_ranges(&mut self, destination_ranges: &[RangeInclusive<u16>]) {
        self.memory.clear_intersecting(destination_ranges);
    }

    pub(super) fn initialize_memory_after_ram_clear(&mut self) {
        self.memory.initialize_after_ram_clear();
    }

    pub(super) fn identity(&self) -> ResetTraceIdentity {
        ResetTraceIdentity {
            address: self.address,
            mapped_prg_bank: self.mapped_prg_bank,
            activation: self.activation.clone(),
        }
    }

    pub(super) fn join_data_state(&self, other: &Self) -> Self {
        debug_assert_eq!(self.identity(), other.identity());
        let mut joined = self.clone();
        joined.accumulator = self.accumulator.union(&other.accumulator);
        joined.index_x = join_value(self.index_x, other.index_x);
        joined.index_y = join_value(self.index_y, other.index_y);
        joined.zero = join_value(self.zero, other.zero);
        joined.negative = join_value(self.negative, other.negative);
        joined.carry = join_value(self.carry, other.carry);
        joined.memory = self.memory.union(&other.memory);
        joined
    }
}

impl ResetTraceIdentity {
    pub(super) fn address(&self) -> u16 {
        self.address
    }

    pub(super) fn mapped_prg_bank(&self) -> Option<u8> {
        self.mapped_prg_bank
    }

    pub(super) fn activation(&self) -> ActivationId {
        self.activation
    }
}

fn join_value<T: Copy + Eq>(left: Option<T>, right: Option<T>) -> Option<T> {
    (left == right).then_some(left).flatten()
}
