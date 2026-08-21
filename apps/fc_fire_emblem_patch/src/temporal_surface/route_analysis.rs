use super::*;

use crate::game_over_dialogue::{
    is_source_selected_game_over_dialogue, source_selected_game_over_dialogue_family_hex,
};

pub(super) const GAME_OVER_DIALOGUE_DIRECTORY_SELECTOR_ADDRESS: usize = 0x77F4;
pub(super) const GAME_OVER_DIALOGUE_ENTRY_SELECTOR_ADDRESS: usize = 0x77F1;

pub(super) fn validate_route_contract(route: &RouteInput) -> Result<()> {
    ensure!(!route.route_role.is_empty(), "temporal route role is empty");
    ensure!(
        !route.entry_action.is_empty(),
        "{} entry action is empty",
        route.route_role
    );
    ensure!(
        !route.source_bound_effect.is_empty(),
        "{} source-bound effect is empty",
        route.route_role
    );
    ensure!(
        route.samples.len() >= MIN_IRREGULAR_SAMPLE_COUNT,
        "{} needs at least {MIN_IRREGULAR_SAMPLE_COUNT} temporal samples",
        route.route_role
    );
    match route.route_role.as_str() {
        "sound_test_shared_battle" => {
            ensure!(
                route.entry_action == "START",
                "sound-test battle must enter through START"
            );
            ensure!(
                !route.negative_case,
                "sound-test battle is not a negative route"
            );
        }
        "sound_test_automatic_ending" => {
            ensure!(
                route.entry_action == "SELECT",
                "sound-test ending must enter through SELECT"
            );
            ensure!(
                !route.negative_case,
                "sound-test ending is not a negative route"
            );
        }
        "gameplay_battle_favorable" => ensure!(
            !route.negative_case,
            "favorable gameplay battle is not a negative route"
        ),
        "gameplay_battle_unfavorable" | "gameplay_battle_defeat" => ensure!(
            route.negative_case,
            "{} must be marked as a negative route",
            route.route_role
        ),
        other => bail!("unknown temporal route role {other}"),
    }
    Ok(())
}

pub(super) fn analyze_route(route: &RouteInput, manifest_root: &Path) -> Result<RouteReport> {
    let frame_offsets = route
        .samples
        .iter()
        .map(|sample| sample.frame_offset)
        .collect::<Vec<_>>();
    ensure!(
        frame_offsets.windows(2).all(|window| window[0] < window[1]),
        "{} frame offsets must be strictly increasing",
        route.route_role
    );
    let frame_deltas = frame_offsets
        .windows(2)
        .map(|window| window[1] - window[0])
        .collect::<BTreeSet<_>>();
    let irregular_temporal_sampling = frame_deltas.len() > 1;
    ensure!(
        irregular_temporal_sampling,
        "{} frame offsets must be irregular rather than a fixed-step sample",
        route.route_role
    );

    let mut samples = Vec::new();
    let mut screen_roles = BTreeSet::new();
    let mut screenshot_sha1s = BTreeSet::new();
    let mut nametable_sha1s = BTreeSet::new();
    let mut oam_sha1s = BTreeSet::new();
    let mut palette_sha1s = BTreeSet::new();
    let mut chr_pairs = BTreeSet::new();
    let mut nametable_tile_codes = BTreeSet::new();
    let mut visible_sprite_tile_codes = BTreeSet::new();
    let mut memory_expectation_count = 0;
    let mut screen_role_variants = BTreeMap::<String, ScreenRoleVariantAccumulator>::new();
    let mut producer_frame_counts = Vec::new();
    let mut capture_dirs = BTreeSet::new();
    let mut game_over_dialogue_selectors_hex = BTreeSet::new();

    for sample in &route.samples {
        validate_screen_role(&route.route_role, &sample.screen_role)?;
        ensure!(
            !sample.expected_memory.is_empty(),
            "{} frame {} has no expected state bytes",
            route.route_role,
            sample.frame_offset
        );
        let capture_dir = resolve_capture_dir(manifest_root, &sample.capture_dir)?;
        ensure!(
            capture_dirs.insert(capture_dir.clone()),
            "{} reuses temporal capture directory {}",
            route.route_role,
            capture_dir.display()
        );
        let files = read_capture_files(&capture_dir)?;
        validate_memory_expectations(sample, &files)?;
        if route.route_role == "gameplay_battle_defeat" && sample.screen_role == "game_over" {
            game_over_dialogue_selectors_hex.insert(validate_game_over_dialogue_selector(&files)?);
        }
        let state = parse_capture_state(&files.state)?;
        let screenshot_sha1 = sha1_hex(&files.screenshot);
        let state_sha1 = sha1_hex(&files.state);
        let internal_ram_sha1 = sha1_hex(&files.internal_ram);
        let prg_ram_sha1 = sha1_hex(&files.prg_ram);
        let nametable_sha1 = sha1_hex(&files.nametable);
        let oam_sha1 = sha1_hex(&files.oam);
        let palette_sha1 = sha1_hex(&files.palette);
        let sample_nametable_tiles = nametable_tile_codes_for(&files.nametable);
        let (sample_sprite_tiles, visible_sprite_count) = visible_sprite_tile_codes_for(&files.oam);

        screen_roles.insert(sample.screen_role.clone());
        screenshot_sha1s.insert(screenshot_sha1.clone());
        nametable_sha1s.insert(nametable_sha1.clone());
        oam_sha1s.insert(oam_sha1.clone());
        palette_sha1s.insert(palette_sha1.clone());
        chr_pairs.insert(state.chr_pair.clone());
        nametable_tile_codes.extend(sample_nametable_tiles);
        visible_sprite_tile_codes.extend(sample_sprite_tiles);
        memory_expectation_count += sample.expected_memory.len();
        producer_frame_counts.push(state.producer_frame_count);
        let role_variant = screen_role_variants
            .entry(sample.screen_role.clone())
            .or_default();
        role_variant.sample_count += 1;
        role_variant.frame_offsets.push(sample.frame_offset);
        role_variant
            .screenshot_sha1s
            .insert(screenshot_sha1.clone());
        role_variant.nametable_sha1s.insert(nametable_sha1.clone());
        role_variant.oam_sha1s.insert(oam_sha1.clone());
        role_variant.palette_sha1s.insert(palette_sha1.clone());

        samples.push(SampleReport {
            frame_offset: sample.frame_offset,
            screen_role: sample.screen_role.clone(),
            producer_frame_count: state.producer_frame_count,
            screenshot_sha1,
            state_sha1,
            internal_ram_sha1,
            prg_ram_sha1,
            nametable_sha1,
            oam_sha1,
            palette_sha1,
            chr_pair: state.chr_pair,
            left_latch: state.left_latch,
            right_latch: state.right_latch,
            background_enabled: state.background_enabled,
            sprites_enabled: state.sprites_enabled,
            background_pattern_address_hex: format!("0x{:04X}", state.background_pattern_address),
            sprite_pattern_address_hex: format!("0x{:04X}", state.sprite_pattern_address),
            visible_sprite_count,
            memory_expectation_count: sample.expected_memory.len(),
        });
    }
    validate_producer_frame_deltas(&route.route_role, &frame_offsets, &producer_frame_counts)?;
    if route.route_role == "gameplay_battle_defeat" {
        ensure!(
            !game_over_dialogue_selectors_hex.is_empty(),
            "defeat route has no game-over dialogue-selector samples"
        );
        ensure!(
            game_over_dialogue_selectors_hex.len() == 1,
            "one defeat route selected multiple game-over dialogues: {}",
            game_over_dialogue_selectors_hex
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    let game_over_dialogue_selector_sample_count = route
        .samples
        .iter()
        .filter(|sample| {
            route.route_role == "gameplay_battle_defeat" && sample.screen_role == "game_over"
        })
        .count();
    let game_over_dialogue_selector_hex = game_over_dialogue_selectors_hex.into_iter().next();
    if route.route_role == "sound_test_automatic_ending" {
        let observed_roles = screen_role_variants
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let expected_roles = ENDING_SCREEN_ROLES.into_iter().collect::<BTreeSet<_>>();
        ensure!(
            observed_roles == expected_roles,
            "automatic ending temporal samples do not cover all five ending screen roles"
        );
        let final_signature = screen_role_variants
            .get("ending_final_signature")
            .context("automatic ending has no final-signature samples")?;
        ensure!(
            final_signature.sample_count >= 4
                && final_signature.frame_offsets.last().unwrap()
                    - final_signature.frame_offsets.first().unwrap()
                    >= 12_000
                && final_signature.screenshot_sha1s.len() == 1
                && final_signature.nametable_sha1s.len() == 1
                && final_signature.oam_sha1s.len() == 1
                && final_signature.palette_sha1s.len() == 1,
            "ending final signature is not stable across the admitted long-span samples"
        );
    }
    let screen_role_variants = screen_role_variants
        .into_iter()
        .map(|(screen_role, variant)| ScreenRoleVariantReport {
            screen_role,
            sample_count: variant.sample_count,
            distinct_screenshot_count: variant.screenshot_sha1s.len(),
            distinct_nametable_count: variant.nametable_sha1s.len(),
            distinct_oam_count: variant.oam_sha1s.len(),
            distinct_palette_count: variant.palette_sha1s.len(),
        })
        .collect();

    Ok(RouteReport {
        route_role: route.route_role.clone(),
        entry_action: route.entry_action.clone(),
        source_bound_effect: route.source_bound_effect.clone(),
        negative_case: route.negative_case,
        sample_count: samples.len(),
        frame_offsets,
        irregular_temporal_sampling,
        screen_roles: screen_roles.into_iter().collect(),
        distinct_screenshot_count: screenshot_sha1s.len(),
        distinct_nametable_count: nametable_sha1s.len(),
        distinct_oam_count: oam_sha1s.len(),
        distinct_palette_count: palette_sha1s.len(),
        memory_expectation_count,
        game_over_dialogue_selector_hex,
        game_over_dialogue_selector_sample_count,
        screen_role_variants,
        chr_pairs: chr_pairs.into_iter().collect(),
        nametable_tile_codes_hex: hex_codes(nametable_tile_codes),
        visible_sprite_tile_codes_hex: hex_codes(visible_sprite_tile_codes),
        samples,
    })
}

pub(super) fn validate_game_over_dialogue_selector(files: &CaptureFiles) -> Result<String> {
    let prg_ram_byte = |address: usize, role: &str| {
        let offset = address
            .checked_sub(MemoryRegion::PrgRam.base_address())
            .with_context(|| format!("game-over {role} is below PRG RAM"))?;
        files
            .prg_ram
            .get(offset)
            .copied()
            .with_context(|| format!("game-over {role} is outside PRG RAM"))
    };
    let directory_selector = prg_ram_byte(
        GAME_OVER_DIALOGUE_DIRECTORY_SELECTOR_ADDRESS,
        "dialogue directory selector",
    )?;
    let entry_selector = prg_ram_byte(
        GAME_OVER_DIALOGUE_ENTRY_SELECTOR_ADDRESS,
        "dialogue entry selector",
    )?;
    ensure!(
        is_source_selected_game_over_dialogue(directory_selector, entry_selector),
        "game-over dialogue selector {directory_selector:02X}:{entry_selector:02X} is outside source-selected family {}",
        source_selected_game_over_dialogue_family_hex()
    );
    Ok(format!("{directory_selector:02X}:{entry_selector:02X}"))
}

pub(super) fn validate_producer_frame_deltas(
    route_role: &str,
    declared_frame_offsets: &[u64],
    producer_frame_counts: &[u64],
) -> Result<()> {
    ensure!(
        declared_frame_offsets.len() == producer_frame_counts.len(),
        "{route_role} declared and producer frame counts have different lengths"
    );
    for (declared, produced) in declared_frame_offsets
        .windows(2)
        .zip(producer_frame_counts.windows(2))
    {
        ensure!(
            produced[0] < produced[1],
            "{route_role} producer frame counts must be strictly increasing"
        );
        ensure!(
            declared[1] - declared[0] == produced[1] - produced[0],
            "{route_role} producer frame delta does not match the declared exact-step delta"
        );
    }
    Ok(())
}

pub(super) fn validate_screen_role(route_role: &str, screen_role: &str) -> Result<()> {
    match route_role {
        "sound_test_automatic_ending" => ensure!(
            ENDING_SCREEN_ROLES.contains(&screen_role),
            "ending route sample has unknown screen role {screen_role}"
        ),
        "gameplay_battle_defeat" => ensure!(
            matches!(screen_role, "battle_animation" | "game_over"),
            "defeat route sample has unknown screen role {screen_role}"
        ),
        _ => ensure!(
            screen_role == "battle_animation",
            "battle route sample has unknown screen role {screen_role}"
        ),
    }
    Ok(())
}

pub(super) fn resolve_capture_dir(manifest_root: &Path, capture_dir: &Path) -> Result<PathBuf> {
    let resolved = if capture_dir.is_absolute() {
        capture_dir.to_path_buf()
    } else {
        manifest_root.join(capture_dir)
    };
    ensure!(
        resolved.is_dir(),
        "temporal capture directory does not exist: {}",
        resolved.display()
    );
    Ok(resolved)
}

pub(super) fn read_capture_files(capture_dir: &Path) -> Result<CaptureFiles> {
    let read = |name: &str| {
        let path = capture_dir.join(name);
        fs::read(&path).with_context(|| format!("read {}", path.display()))
    };
    let files = CaptureFiles {
        screenshot: read("screenshot.png")?,
        state: read("state.json")?,
        internal_ram: read("iram.bin")?,
        prg_ram: read("prgram.bin")?,
        nametable: read("nametable.bin")?,
        oam: read("oam.bin")?,
        palette: read("palette.bin")?,
    };
    ensure!(
        files.screenshot.starts_with(b"\x89PNG\r\n\x1A\n"),
        "{} screenshot is not PNG",
        capture_dir.display()
    );
    ensure!(
        files.internal_ram.len() == INTERNAL_RAM_BYTE_COUNT,
        "{} internal RAM dump length changed",
        capture_dir.display()
    );
    ensure!(
        files.prg_ram.len() == PRG_RAM_BYTE_COUNT,
        "{} PRG RAM dump length changed",
        capture_dir.display()
    );
    ensure!(
        files.nametable.len() == NAMETABLE_BYTE_COUNT,
        "{} nametable dump length changed",
        capture_dir.display()
    );
    ensure!(
        files.oam.len() == OAM_BYTE_COUNT,
        "{} OAM dump length changed",
        capture_dir.display()
    );
    ensure!(
        files.palette.len() == PALETTE_BYTE_COUNT,
        "{} palette dump length changed",
        capture_dir.display()
    );
    Ok(files)
}

pub(super) fn validate_memory_expectations(
    sample: &SampleInput,
    files: &CaptureFiles,
) -> Result<()> {
    for expectation in &sample.expected_memory {
        ensure!(
            !expectation.reason.is_empty(),
            "frame {} has an expected byte range without a reason",
            sample.frame_offset
        );
        let expected = decode_hex(&expectation.bytes_hex).with_context(|| {
            format!(
                "decode frame {} expected {} bytes",
                sample.frame_offset,
                expectation.region.file_name()
            )
        })?;
        ensure!(
            !expected.is_empty(),
            "frame {} expected byte range is empty",
            sample.frame_offset
        );
        let region = match expectation.region {
            MemoryRegion::InternalRam => &files.internal_ram,
            MemoryRegion::PrgRam => &files.prg_ram,
        };
        let base = expectation.region.base_address();
        ensure!(
            expectation.address >= base,
            "frame {} expected address 0x{:04X} is below {}",
            sample.frame_offset,
            expectation.address,
            expectation.region.file_name()
        );
        let offset = expectation.address - base;
        let end = offset
            .checked_add(expected.len())
            .context("expected memory range overflow")?;
        ensure!(
            end <= expectation.region.byte_count() && end <= region.len(),
            "frame {} expected range crosses {}",
            sample.frame_offset,
            expectation.region.file_name()
        );
        ensure!(
            region[offset..end] == expected,
            "frame {} expected bytes at 0x{:04X} changed ({})",
            sample.frame_offset,
            expectation.address,
            expectation.reason
        );
    }
    Ok(())
}

pub(super) fn decode_hex(value: &str) -> Result<Vec<u8>> {
    ensure!(
        value.len().is_multiple_of(2),
        "hex byte string has odd length"
    );
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .with_context(|| format!("invalid hex byte at character {index}"))
        })
        .collect()
}

pub(crate) fn nametable_tile_codes_for(nametable: &[u8]) -> BTreeSet<u8> {
    (0..2)
        .flat_map(|page| {
            let start = page * NAMETABLE_PAGE_BYTE_COUNT;
            nametable[start..start + NAMETABLE_TILE_BYTE_COUNT]
                .iter()
                .copied()
        })
        .collect()
}

pub(crate) fn visible_sprite_tile_codes_for(oam: &[u8]) -> (BTreeSet<u8>, usize) {
    let visible_sprites = oam
        .chunks_exact(4)
        .filter(|sprite| sprite[0] <= VISIBLE_SPRITE_Y_MAX)
        .collect::<Vec<_>>();
    let tile_codes = visible_sprites.iter().map(|sprite| sprite[1]).collect();
    (tile_codes, visible_sprites.len())
}

pub(crate) fn hex_codes(codes: BTreeSet<u8>) -> Vec<String> {
    codes
        .into_iter()
        .map(|code| format!("{code:02X}"))
        .collect()
}
