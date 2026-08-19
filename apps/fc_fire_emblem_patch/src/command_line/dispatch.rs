use crate::*;

pub(super) fn execute(command: Command) -> Result<()> {
    match command {
        Command::VerifySource { source } => {
            let source_rom = rom::Rom::from_path(&source)?;
            source_rom.verify_supported_japanese()?;
            println!(
                "verified Japanese source: SHA-1 {} (mapper {}, PRG {} bytes, CHR {} bytes)",
                rom::EXPECTED_SOURCE_SHA1,
                source_rom.mapper(),
                source_rom.prg().len(),
                source_rom.chr().len()
            );
        }
        Command::BuildReleaseImage {
            cumulative,
            output,
            report,
        } => {
            let cumulative_rom = rom::Rom::from_path(&cumulative)?;
            let (image, plan) = release_image::build_release_image(&cumulative_rom)?;
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&output, &image)?;
            let json = serde_json::to_string_pretty(&plan)?;
            std::fs::write(&report, format!("{json}\n"))?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", plan.output_sha1);
            println!(
                "release image: {} header, mapper {}, PRG {} bytes, CHR {} -> {} bytes ({} zero pages appended), declares {} bytes CHR RAM and {} bytes battery work RAM",
                plan.header_format,
                plan.mapper,
                plan.prg_byte_count,
                plan.input_chr_byte_count,
                plan.output_chr_byte_count,
                plan.appended_zero_chr_page_count,
                plan.chr_ram_byte_count,
                plan.battery_work_ram_byte_count
            );
        }
        Command::AnalyzeFontSupply {
            source,
            report,
            sheet,
            scale,
        } => {
            let summary = chr_inventory::analyze_font_supply(&source, &report, &sheet, scale)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("wrote {}", sheet.display());
            println!(
                "CHR pages: {}, protected font codes: {}, unresolved font codes: {}",
                summary.page_count, summary.protected_code_count, summary.unresolved_code_count
            );
        }
        Command::AnalyzeTextTables { source, report } => {
            let summary = text_inventory::analyze_text_tables(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "text tables: {}, pointers: {}, unique strings: {}, protected original bytes: {}",
                summary.table_count,
                summary.pointer_count,
                summary.unique_string_count,
                summary.referenced_protected_original_byte_count
            );
        }
        Command::ExtractFixedTextWorkspace { source, output } => {
            let summary = text_inventory::extract_fixed_text_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "fixed text: {} unique entries, {} Japanese-bearing, {} translations preserved",
                summary.entry_count,
                summary.japanese_entry_count,
                summary.preserved_translation_count
            );
        }
        Command::ExtractFrontEndMenuWorkspace { source, output } => {
            let summary = front_end_menu::extract_front_end_menu_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "front-end menu: {} entries, {} translations preserved",
                summary.entry_count, summary.preserved_translation_count
            );
        }
        Command::ExtractUnitNameWorkspace { source, output } => {
            let summary = text_inventory::extract_unit_name_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "unit names: {} entries, {} Japanese entries, {} translations preserved",
                summary.entry_count,
                summary.japanese_entry_count,
                summary.preserved_translation_count
            );
        }
        Command::ExtractLocationNameWorkspace { source, output } => {
            let summary = text_inventory::extract_location_name_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "location names: {} entries, {} Japanese entries, {} translations preserved",
                summary.entry_count,
                summary.japanese_entry_count,
                summary.preserved_translation_count
            );
        }
        Command::ExtractClassProfileWorkspace { source, output } => {
            let summary = class_profile::extract_class_profile_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "class profiles: {} entries, {} description lines, {} translations preserved",
                summary.entry_count,
                summary.description_line_count,
                summary.preserved_translation_count
            );
        }
        Command::AnalyzeBattleTextWorkset {
            source,
            fixed_workspace,
            dialogue_workspace,
            report,
        } => {
            let summary = battle_text_workset::analyze_battle_text_workset(
                &source,
                &fixed_workspace,
                &dialogue_workspace,
                &report,
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle glyphs: fixed {}, dialogue {}, union {}, conservative combination upper bound {}",
                summary.fixed_glyph_count,
                summary.dialogue_glyph_count,
                summary.union_glyph_count,
                summary.conservative_combination_upper_bound
            );
        }
        Command::AnalyzeBattleCodebookPlan {
            source,
            fixed_workspace,
            dialogue_workspace,
            report,
        } => {
            let summary = mapper165::battle_codebook_plan::analyze_battle_codebook_plan(
                &source,
                &fixed_workspace,
                &dialogue_workspace,
                &report,
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle stable codebook: {} glyphs, {} conflicts, clique lower bound {}, coloring upper bound {}, {} chapter-one-safe codes",
                summary.glyph_count,
                summary.conflict_edge_count,
                summary.constructed_clique_glyph_count,
                summary.stable_color_count,
                summary.chapter_one_safe_code_count
            );
        }
        Command::AnalyzeBattleSurfaceConstraints {
            source,
            fixed_workspace,
            dialogue_workspace,
            temporal_manifest,
            report,
        } => {
            let summary = mapper165::battle_codebook_plan::surface_constraints::analyze_battle_surface_constraints(
                &source,
                &fixed_workspace,
                &dialogue_workspace,
                &temporal_manifest,
                &report,
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle surface constraints: {} samples, {} runtime tuples, maximum {} selected colors, maximum {} remap pairs, assignment catalog {}",
                summary.sample_count,
                summary.runtime_tuple_count,
                summary.maximum_selected_color_count,
                summary.maximum_remap_pair_count,
                summary.dynamic_assignment_catalog_sha1,
            );
        }
        Command::BuildBattleTextCacheBase {
            source,
            fixed_workspace,
            dialogue_workspace,
            output,
            report,
        } => {
            let summary = mapper165::battle_text_cache_probe::build_battle_text_cache_base(
                &source,
                &fixed_workspace,
                &dialogue_workspace,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle glyph atlas: {} glyphs, {} bytes",
                summary.glyph_count, summary.glyph_atlas_byte_count
            );
        }
        Command::BuildBattleTextRuntimeBase {
            source,
            fixed_workspace,
            dialogue_workspace,
            temporal_manifest,
            output,
            report,
        } => {
            let summary = mapper165::battle_text_runtime_base::build_battle_text_runtime_base(
                &source,
                &fixed_workspace,
                &dialogue_workspace,
                &temporal_manifest,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle runtime text base: {} fixed entries, {} dialogue records, {} tracked text writes",
                summary.fixed_entry_count,
                summary.dialogue_record_count,
                summary.tracked_write_count
            );
        }
        Command::BuildBattleCompositionLoaderProbe {
            source,
            temporal_manifest,
            base,
            base_report,
            output,
            report,
        } => {
            let summary =
                mapper165::battle_composition_loader_probe::build_battle_composition_loader_probe(
                    &source,
                    &temporal_manifest,
                    &base,
                    &base_report,
                    &output,
                    &report,
                )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle composition loader: {} observed verification tuples, at most {} observed PPU data writes, {} runtime bytes",
                summary.observed_runtime_tuple_count,
                summary.maximum_observed_ppu_write_count,
                summary.runtime_routine_byte_count
            );
        }
        Command::VerifyBattleCompositionRuntime { rom, event, report } => {
            let summary =
                mapper165::battle_composition_runtime_verify::verify_battle_composition_runtime(
                    &rom, &event, &report,
                )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle runtime composition: expected {}, actual {}, {} differing bytes across {} tiles",
                summary.expected_chr_ram_sha1,
                summary.actual_chr_ram_sha1,
                summary.differing_byte_count,
                summary.differing_tile_count
            );
        }
        Command::BuildBattleCombinationProbe {
            source,
            fixed_workspace,
            dialogue_workspace,
            output,
            report,
        } => {
            let summary =
                mapper165::battle_combination_probe::build_gameplay_battle_combination_probe(
                    &source,
                    &fixed_workspace,
                    &dialogue_workspace,
                    &output,
                    &report,
                )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "gameplay battle combination: {} glyphs, {} tracked writes",
                summary.glyph_count, summary.tracked_write_count
            );
        }
        Command::BuildBattleCacheUploadProbe {
            source,
            fixed_workspace,
            dialogue_workspace,
            output,
            report,
        } => {
            let summary = mapper165::battle_cache_upload_probe::build_battle_cache_upload_probe(
                &source,
                &fixed_workspace,
                &dialogue_workspace,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle cache upload: {} glyphs, {} runtime writes",
                summary.glyph_count, summary.runtime_tracked_write_count
            );
        }
        Command::AnalyzeDialogueStructure { source, report } => {
            let summary = dialogue_inventory::analyze_dialogue_structure(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "dialogue tables: {}, pointers: {}, unique targets: {}, alias groups: {}",
                summary.table_count,
                summary.pointer_count,
                summary.unique_target_count,
                summary.alias_group_count
            );
        }
        Command::AnalyzeScreenContracts { source, report } => {
            let summary = screen_contracts::analyze_screen_contracts(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "screen contracts: {}, runtime observed: {}, mixed original-Latin: {}, next observation gate: {}",
                summary.screen_count,
                summary.runtime_observed_screen_count,
                summary.mixed_original_latin_screen_count,
                summary.next_observation_gate_role
            );
        }
        Command::AnalyzeTranslationCoverage(command) => command.execute()?,
        Command::PlanFullTranslationInstallation(command) => command.execute()?,
        Command::AnalyzeChapterTransitions { source, report } => {
            let summary = chapter_transition::analyze_chapter_transitions(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "chapter transitions: {} observed screens, {} chapter contexts, {} chapter titles, {} intro runtime samples, {} source-bound regions, next observation gate: {}",
                summary.screen_count,
                summary.chapter_context_count,
                summary.chapter_title_count,
                summary.chapter_intro_runtime_sample_count,
                summary.source_region_count,
                summary.next_observation_gate_role
            );
        }
        Command::ExtractChapterTitleWorkspace { source, output } => {
            let summary = chapter_transition::extract_chapter_title_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "chapter titles: {} entries, {} Japanese-bearing, {} translations preserved",
                summary.entry_count,
                summary.japanese_entry_count,
                summary.preserved_translation_count
            );
        }
        Command::AnalyzeTemporalSurfaces {
            source,
            manifest,
            report,
        } => {
            let summary = temporal_surface::analyze_temporal_surfaces(&source, &manifest, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "temporal surfaces: {} routes, {} samples, {} CHR pairs, required route coverage complete: {}",
                summary.route_count,
                summary.sample_count,
                summary.chr_pair_count,
                summary.required_route_coverage_complete
            );
        }
        Command::AnalyzeEpilogueVariants {
            source,
            captures,
            capture_rom,
            mapper_report,
            report,
        } => {
            let summary = epilogue_variant_evidence::analyze_epilogue_variants(
                &source,
                &capture_rom,
                &mapper_report,
                &captures,
                &report,
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "epilogue variants: {} visible entries, {} irregular samples, {} CHR pairs, evidence complete: {}",
                summary.visible_entry_count,
                summary.sample_count,
                summary.chr_pair_count,
                summary.evidence_complete
            );
        }
        Command::AnalyzeChapterVictory { source, report } => {
            let summary = chapter_victory::analyze_chapter_victory(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "chapter victory route: {} source castle tiles, {} source-bound regions, {} route steps, {} runtime screens, continuous gate closed: {}, next observation gate: {}",
                summary.victory_tile_count,
                summary.source_region_count,
                summary.route_step_count,
                summary.runtime_screen_count,
                summary.continuous_gate_closed,
                summary.next_observation_gate
            );
        }
        Command::AnalyzeGlyphDemand {
            main_dialogue_workspace,
            fixed_text_workspace,
            populations,
            coresident_sets,
            slot_budget,
            report,
        } => {
            let summary = glyph_demand::analyze_glyph_demand(
                &main_dialogue_workspace,
                &fixed_text_workspace,
                &populations,
                &coresident_sets,
                slot_budget,
                &report,
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "glyph demand: {} populations, {} co-resident sets, {} over the {}-slot budget{}",
                summary.population_count,
                summary.coresident_set_count,
                summary.over_budget_set_names.len(),
                slot_budget,
                if summary.over_budget_set_names.is_empty() {
                    String::new()
                } else {
                    format!(": {}", summary.over_budget_set_names.join(", "))
                }
            );
        }
        Command::AnalyzeShopFlow { source, report } => {
            let summary = shop_flow::analyze_shop_flow(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "shop flow: {} observed screens, {} source-bound regions, next: {}",
                summary.screen_count, summary.source_region_count, summary.next_screen_role
            );
        }
        Command::AnalyzeItemFlow { source, report } => {
            let summary = item_flow::analyze_item_flow(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "item flow: {} screen roles, {} source-bound regions, {} action choices, {} usable items, next: {}",
                summary.screen_count,
                summary.source_region_count,
                summary.action_count,
                summary.usable_item_count,
                summary.next_screen_role
            );
        }
        Command::AnalyzeUnitUiText {
            source,
            fixed_text_workspace,
            unit_name_localization,
            unit_ui_label_localization,
            report,
        } => {
            let summary = unit_ui_text::analyze_unit_ui_text(
                &source,
                &fixed_text_workspace,
                &unit_name_localization,
                &unit_ui_label_localization,
                &report,
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "unit UI text: {} screen roles, {} composers, {} fixed labels, {} Japanese labels targeted, {} command labels, {} dynamic pointers / {} unique strings, Hangul ceiling {}, single-family fit {}",
                summary.screen_role_count,
                summary.composer_count,
                summary.fixed_label_count,
                summary.translated_japanese_label_count,
                summary.command_label_count,
                summary.dynamic_pointer_count,
                summary.dynamic_unique_string_count,
                summary.provisional_hangul_slot_ceiling,
                summary.single_family_page_fit
            );
        }
        Command::ExtractMainDialogueSource { source, output } => {
            let summary = dialogue_assets::extract_main_dialogue_source(&source, &output)?;
            println!("wrote {}", output.display());
            println!("asset SHA-1: {}", summary.asset_sha1);
            println!(
                "main dialogue source: {} regions, {} records, {} unique storage bytes",
                summary.storage_region_count,
                summary.record_count,
                summary.unique_storage_byte_count
            );
        }
        Command::ExtractMainDialogueWorkspace { source, output } => {
            let summary = dialogue_assets::extract_main_dialogue_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "main dialogue workspace: {} records, {} lines, {} safe Japanese source bytes, {} relocation-blocked lines, {} preserved translations",
                summary.record_count,
                summary.line_count,
                summary.safe_japanese_source_byte_count,
                summary.blocked_line_count,
                summary.preserved_translation_line_count
            );
        }
        Command::ExtractBattleDialogueWorkspace { source, output } => {
            let summary = dialogue_assets::extract_battle_dialogue_workspace(&source, &output)?;
            println!("wrote {}", output.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "battle dialogue workspace: {} records, {} lines, {} Japanese source bytes, {} preserved translations",
                summary.record_count,
                summary.line_count,
                summary.japanese_source_byte_count,
                summary.preserved_translation_line_count
            );
        }
        Command::ValidateBattleDialogueWorkspace { source, workspace } => {
            let summary = dialogue_assets::validate_battle_dialogue_workspace(&source, &workspace)?;
            println!("validated {}", workspace.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "battle dialogue translations: {} records, {} lines, {} filled, {} complete, {} target glyphs, {} translated-record bytes + {} preserved bytes = {} planned bytes, {} bytes remaining",
                summary.record_count,
                summary.line_count,
                summary.filled_line_count,
                summary.complete_line_count,
                summary.target_glyph_count,
                summary.translated_record_storage_byte_count,
                summary.preserved_unreferenced_storage_byte_count,
                summary.planned_storage_byte_count,
                summary.remaining_storage_byte_count
            );
        }
        Command::ImportBattleDialogueDraft { workspace, draft } => {
            let summary = dialogue_assets::import_battle_dialogue_draft(&workspace, &draft)?;
            println!("updated {}", workspace.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "imported {} battle dialogue draft lines",
                summary.imported_line_count
            );
        }
        Command::PlanBattleDialogueReinsertion {
            source,
            workspace,
            report,
        } => {
            let summary =
                dialogue_assets::plan_battle_dialogue_reinsertion(&source, &workspace, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle dialogue layout: {} records, {} pointer writes, {} translated bytes + {} preserved bytes, {} bytes remaining",
                summary.record_count,
                summary.pointer_write_count,
                summary.translated_record_storage_byte_count,
                summary.preserved_storage_byte_count,
                summary.remaining_storage_byte_count
            );
        }
        Command::BuildBattleDialogueProbe {
            source,
            workspace,
            output,
            report,
        } => {
            let summary = mapper165::battle_dialogue_probe::build_battle_dialogue_probe(
                &source, &workspace, &output, &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "battle dialogue probe: {} records, {} translated lines, {} pointer writes, {} unique glyphs, {} tracked writes",
                summary.record_count,
                summary.translated_line_count,
                summary.pointer_write_count,
                summary.unique_glyph_count,
                summary.tracked_write_count
            );
        }
        Command::ValidateMainDialogueWorkspace { source, workspace } => {
            let summary = dialogue_assets::validate_main_dialogue_workspace(&source, &workspace)?;
            println!("validated {}", workspace.display());
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!(
                "main dialogue translations: {} records, {} lines, {} filled, {} complete, {} source-preserved, {} untranslated Japanese, {} target glyphs, input complete: {}, review complete: {}",
                summary.record_count,
                summary.line_count,
                summary.filled_line_count,
                summary.complete_line_count,
                summary.preserved_source_line_count,
                summary.untranslated_japanese_line_count,
                summary.target_glyph_count,
                summary.translation_input_complete,
                summary.review_complete
            );
        }
        Command::AnalyzeMainDialogueGlyphWorkset {
            source,
            workspace,
            maximum_lifetime_evidence,
            report,
        } => {
            let summary = dialogue_assets::analyze_main_dialogue_glyph_workset(
                &source,
                &workspace,
                &maximum_lifetime_evidence,
                &report,
            )?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "main dialogue glyph workset: {} filled lines, {} complete lines, {} filled unique glyphs, {} approved unique glyphs, max transition chain {} glyphs, chains fit one page: {}, max observed screen lifetime {} slots, observed lifetimes fit one page: {}, draft ready: {}",
                summary.filled_line_count,
                summary.complete_line_count,
                summary.filled_unique_glyph_count,
                summary.approved_unique_glyph_count,
                summary.max_transition_chain_unique_glyph_count,
                summary.filled_transition_chains_fit_one_page,
                summary.max_observed_screen_lifetime_slot_demand,
                summary.filled_observed_screen_lifetimes_fit_one_page,
                summary.working_set_ready
            );
        }
        Command::PlanMainDialogueReinsertion {
            source,
            workspace,
            report,
        } => {
            let summary =
                dialogue_assets::plan_main_dialogue_reinsertion(&source, &workspace, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "main dialogue layout: {} regions, {} records, {} pointer writes, {} planned bytes, {} remaining bytes, {} changed records, translation input complete: {}, review complete: {}, release eligible: {}",
                summary.region_count,
                summary.record_count,
                summary.pointer_write_count,
                summary.planned_storage_byte_count,
                summary.remaining_storage_byte_count,
                summary.changed_record_count,
                summary.translation_input_complete,
                summary.review_complete,
                summary.release_eligible
            );
        }
        Command::SummarizeMainDialoguePageBoundaryTopology {
            source,
            workspace,
            record_id,
        } => {
            let summary = dialogue_assets::summarize_main_dialogue_page_boundary_topology(
                &source, &workspace, &record_id,
            )?;
            println!("workspace SHA-1: {}", summary.workspace_sha1);
            println!("record: {}", summary.record_id);
            println!("page-boundary topology SHA-1: {}", summary.topology_sha1);
            println!(
                "source pointer: 0x{:04X}; logical bytes: {}; lines: {}",
                summary.source_pointer_cpu_address, summary.logical_byte_count, summary.line_count
            );
        }
        Command::VerifyMaximumDialogueBoundaryRebinding {
            source,
            reference_output,
            candidate_output,
            workspace,
            page_boundaries,
        } => {
            let summary =
                mapper165::maximum_dialogue_rebinding::verify_maximum_dialogue_boundary_rebinding(
                    &source,
                    &workspace,
                    &page_boundaries,
                    &reference_output,
                    &candidate_output,
                )?;
            println!("reference output SHA-1: {}", summary.reference_output_sha1);
            println!("candidate output SHA-1: {}", summary.candidate_output_sha1);
            println!(
                "page-boundary topology SHA-1: {}",
                summary.record_page_boundary_topology_sha1
            );
            println!(
                "verified rendered maximum dialogue: {} pages, {} logical bytes, {} target-glyph bytes",
                summary.page_count, summary.logical_byte_count, summary.target_glyph_byte_count
            );
        }
        Command::VerifyMainDialogueSourceRoundtrip {
            source,
            asset,
            output,
        } => {
            let summary =
                dialogue_assets::verify_main_dialogue_source_roundtrip(&source, &asset, &output)?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!(
                "verified exact source roundtrip: {} regions, {} records",
                summary.storage_region_count, summary.record_count
            );
        }
        Command::BuildTitleLogoAsset {
            source,
            manifest,
            asset,
            preview,
            report,
        } => {
            let summary = title_graphics::build_title_logo_asset(
                &source, &manifest, &asset, &preview, &report,
            )?;
            println!("wrote {}", asset.display());
            println!("asset SHA-1: {}", summary.asset_sha1);
            println!("wrote {}", preview.display());
            println!("preview SHA-1: {}", summary.preview_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "title logo: {} unique target tiles in {} source-owned slots",
                summary.target_unique_nonblank_tile_count, summary.source_owned_tile_count
            );
        }
        Command::BuildOptionsPoc {
            source,
            localization,
            output,
            preview,
            preview_scale,
        } => {
            let report = options::build_options_poc(
                &source,
                &localization,
                &output,
                &preview,
                preview_scale,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", report.output_sha1);
            println!("wrote {}", preview.display());
            for write in report.writes {
                println!(
                    "tracked write: {} at {:#08X} ({} bytes)",
                    write.label, write.offset, write.len
                );
            }
        }
        Command::BuildMapper165ParityProbe {
            source,
            output,
            report,
        } => {
            let summary = mapper165::build_mapper165_parity_probe(&source, &output, &report)?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("tracked ROM writes: {}", summary.tracked_write_count);
        }
        Command::PlanHangulPageProof {
            source,
            localization,
            page_pack,
            report,
        } => {
            let summary = hangul_page_plan::plan_hangul_page_proof(
                &source,
                &localization,
                &page_pack,
                &report,
            )?;
            println!("wrote {}", page_pack.display());
            println!("page pack SHA-1: {}", summary.page_pack_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "Hangul pages: {} active slots, {}-glyph proof union, {} extension pages available",
                summary.active_hangul_slot_count,
                summary.page_union_glyph_count,
                summary.maximum_extension_page_count
            );
        }
        Command::BuildMapper165HangulPageProbe {
            source,
            localization,
            roster_localization,
            options_screen_evidence,
            output,
            report,
        } => {
            let summary = mapper165::hangul_page_probe::build_mapper165_hangul_page_probe(
                &source,
                &localization,
                &roster_localization,
                &options_screen_evidence,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("options page pack SHA-1: {}", summary.page_pack_sha1);
            println!("roster page pack SHA-1: {}", summary.roster_page_pack_sha1);
            println!("tracked ROM writes: {}", summary.tracked_write_count);
        }
        Command::BuildKrPatch(command) => command.execute()?,
        Command::BuildMainDialogueSliceProbe {
            source,
            workspace,
            screen_evidence,
            record_id,
            output,
            report,
        } => {
            let summary = mapper165::dialogue_slice_probe::build_dialogue_slice_probe(
                &source,
                &workspace,
                &screen_evidence,
                &record_id,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "dialogue slice: {} lines, {} unique glyphs, {} planned bytes, {} bytes remaining, {} preserved active codes, {} temporal samples, {} tracked writes",
                summary.translated_line_count,
                summary.unique_glyph_count,
                summary.planned_storage_byte_count,
                summary.remaining_storage_byte_count,
                summary.preserved_active_code_count,
                summary.temporal_sample_count,
                summary.tracked_write_count
            );
        }
        Command::AnalyzeMapper165TriggerPlanes { source, report } => {
            let summary =
                mapper165::trigger_planes::analyze_mapper165_trigger_planes(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("observed screens: {}", summary.observed_screen_count);
            println!("unique FD/FE pairs: {}", summary.unique_pair_count);
            println!(
                "required CHR variant pages: {}",
                summary.required_variant_page_count
            );
            println!(
                "pair-aware selector required: {}",
                summary.pair_aware_selector_required
            );
        }
        Command::AnalyzeMapper165DirectChrPairs { source, report } => {
            let summary = mapper165::direct_chr_pairs::analyze_direct_chr_pairs(&source, &report)?;
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("direct CHR writers: {}", summary.direct_writer_count);
            println!(
                "same-value paired writers: {}",
                summary.same_value_writer_count
            );
            println!(
                "immediate left-FD writers: {}",
                summary.immediate_left_fd_writer_count
            );
            println!(
                "writers requiring runtime co-lifetime observation: {}",
                summary.runtime_observation_writer_count
            );
        }
        Command::BuildMmc5PrgProbe {
            source,
            output,
            report,
        } => {
            let summary = mmc5_prg::build_mmc5_prg_probe(&source, &output, &report)?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("tracked ROM writes: {}", summary.tracked_write_count);
        }
        Command::BuildMmc5ChrWriterProbe {
            source,
            output,
            report,
        } => {
            let summary = mmc5_chr::build_mmc5_chr_writer_probe(&source, &output, &report)?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "tracked writes after PRG probe: {}",
                summary.tracked_delta_write_count
            );
        }
        Command::BuildMmc5ExpandedChrOptionsProbe {
            source,
            localization,
            output,
            report,
        } => {
            let summary = mmc5_expanded_chr::build_mmc5_expanded_chr_options_probe(
                &source,
                &localization,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!("tracked ROM writes: {}", summary.tracked_write_count);
        }
        Command::BuildMmc5DialogueExramProbe {
            source,
            attributes,
            output,
            report,
        } => {
            let summary = mmc5_exram_probe::build_mmc5_dialogue_exram_probe(
                &source,
                &attributes,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "tracked writes after CHR writer probe: {}",
                summary.tracked_write_count
            );
        }
        Command::BuildMmc5NametableShadowProbe {
            source,
            output,
            report,
        } => {
            let summary = mmc5_nametable_shadow::build_mmc5_nametable_shadow_probe(
                &source, &output, &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "hooked direct PPU stores: {}, tracked writes after CHR writer probe: {}",
                summary.hooked_store_count, summary.tracked_write_count
            );
        }
        Command::BuildMmc5QueueShadowProbe {
            source,
            output,
            report,
        } => {
            let summary =
                mmc5_queue_shadow::build_mmc5_queue_shadow_probe(&source, &output, &report)?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "tracked writes after CHR writer probe: {}",
                summary.tracked_write_count
            );
        }
        Command::ProjectMmc4LatchNametable {
            input,
            nametable_index,
            fd_bank,
            fe_bank,
            initial_latch,
            output,
            report,
        } => {
            let summary = mmc4_latch::project_mmc4_latch_nametable(
                &input,
                nametable_index,
                fd_bank,
                fe_bank,
                initial_latch,
                &output,
                &report,
            )?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "MMC4 latch triggers: FD {}, FE {}, ending latch {}",
                summary.fd_trigger_count, summary.fe_trigger_count, summary.ending_latch
            );
        }
        Command::ReplayMmc4LatchPpuTransfers {
            input,
            output,
            report,
        } => {
            let summary = mmc4_latch::replay_mmc4_latch_ppu_transfers(&input, &output, &report)?;
            println!("wrote {}", output.display());
            println!("output SHA-1: {}", summary.output_sha1);
            println!("wrote {}", report.display());
            println!("report SHA-1: {}", summary.report_sha1);
            println!(
                "applied nametable writes: {}",
                summary.nametable_write_count
            );
        }
    }
    Ok(())
}
