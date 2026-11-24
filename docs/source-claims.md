# Source claims

Each claim the README's [Game sources](../README.md#game-sources) section makes, and what covers it (#825). A change to
a source or to that section updates this table in the same pull request.

- **Test:** a test in `crates/pindu-core`. `import/` is `tests/import/`, which runs against the fixtures in
  `tests/fixtures/` in native and Flatpak layouts; `src/` is a unit test in that file.
- **Hardware:** a row of the [hardware checklist](releasing.md#hardware-checklist), run by hand for every release.
- **Hand check:** checked by hand on 2026-11-24, with how. Repeat it when the code it names changes.

External importers are left out: their protocol has its own contract (#547).

## Steam

| Claim | Covered by |
| --- | --- |
| Native or Flatpak | Test: `import/steam.rs` `librarycache_art_is_used_in_every_layout`, `flatpak_steam_imports_its_main_library_whatever_path_it_records`, `release_year_genres_and_controller_support_come_from_appinfo_in_both_layouts` |
| Installed games, and games still downloading as not installed | Test: `import/steam.rs` `install_state_comes_from_the_manifest_state_flags`, `every_library_in_libraryfolders_is_read`, `a_library_outside_granted_paths_is_read_from_its_apps_list`, `non_games_are_filtered_and_platform_and_developer_come_from_appinfo` |
| With their playtime | Test: `import/steam.rs` `playtime_comes_from_the_most_recent_accounts_localconfig`, `the_other_account_counts_when_it_was_used_last`; `src/sources/sync.rs` `the_sources_playtime_is_stored_and_refreshed` |
| With their Steam Deck rating | Test: `import/steam.rs` `the_steam_deck_rating_comes_from_appinfo`, `without_appinfo_there_is_no_steam_deck_rating`; `src/sources/sync.rs` `the_steam_deck_rating_is_the_sources_value_and_follows_each_sync` |
| Start through Steam | Test: `src/launch.rs` `steam_games_start_by_app_id`. Hardware: Steam game |

## Heroic

| Claim | Covered by |
| --- | --- |
| Native or Flatpak | Test: `import/heroic.rs` `native_and_flatpak_heroic_both_import`, `amazon_release_year_comes_from_its_release_date_in_both_layouts` |
| Epic Games, GOG, Amazon and sideloaded games | Test: `import/heroic.rs` `epic_library_is_joined_with_its_installed_json`, `gog_platform_comes_from_install_info`, `amazon_games_are_joined_by_id_and_run_on_windows`, `sideloaded_games_use_their_own_install_info_and_local_art` |
| Installed or not | Test: `import/heroic.rs` `epic_library_is_joined_with_its_installed_json`, `gog_platform_comes_from_install_info` |
| Start through Heroic | Test: `src/launch.rs` `heroic_games_name_their_runner`, `app_names_are_percent_encoded`. Hardware: Game from another launcher |

## Lutris

| Claim | Covered by |
| --- | --- |
| Native or Flatpak | Test: `import/lutris.rs` `native_and_flatpak_lutris_import_the_same_games`, `a_game_takes_its_service_runner_and_year_in_both_layouts` |
| Lutris games | Test: `import/lutris.rs` `a_game_takes_its_details_playtime_and_art`, `games_without_art_playtime_or_install_import_without_them` |
| Start through Lutris | Test: `src/launch.rs` `lutris_games_start_by_their_lutris_id`. Hardware: Game from another launcher |

## Bottles

Bottles is read by the bundled `pindu-import-bottles` helper. `crates/pindu-import-bottles/tests/import/` runs the same
`import/bottles.rs` against the built helper, so the tests below cover the helper and the in-process importer alike.

| Claim | Covered by |
| --- | --- |
| Native or Flatpak | Test: `import/bottles.rs` `native_and_flatpak_bottles_import_the_same_programs` |
| Programs in your bottles | Test: `import/bottles.rs` `native_and_flatpak_bottles_import_the_same_programs`, `steam_and_umu_entries_are_left_to_their_own_sources` |
| Start through Bottles | Test: `import/bottles.rs` `a_program_starts_through_bottles_run`; `src/launch.rs` `bottles_programs_start_by_the_uri_their_source_gave`. Hardware: Game from another launcher |

## Faugus Launcher

| Claim | Covered by |
| --- | --- |
| Native or Flatpak | Test: `import/faugus.rs` `native_and_flatpak_faugus_import_the_same_games` |
| Faugus games | Test: `import/faugus.rs` `a_game_takes_its_details_playtime_and_art`, `unplayed_and_uncategorized_games_import_without_those` |
| Start on the host, which needs host access | Test: `import/faugus.rs` `native_faugus_games_start_through_faugus_on_the_host`, `flatpak_faugus_games_start_through_its_flatpak_command`; `src/launch.rs` `desktop_entries_and_host_commands_spawn_their_argv_on_the_host`; `src/permissions.rs` `only_games_we_start_on_the_host_need_host_spawn` |

## Legendary

| Claim | Covered by |
| --- | --- |
| The standalone client, native only | Test: `import/legendary.rs` `native_legendary_is_found_in_the_config_folder`, `inside_flatpak_the_hosts_config_folder_is_the_candidate` |
| Installed Epic Games Store games | Test: `import/legendary.rs` `installed_games_import_and_dlc_is_skipped`, `a_game_takes_its_install_and_metadata` |
| That Heroic does not already list | Test: `src/sources/sync.rs` `standalone_legendary_skips_the_games_heroic_lists` |
| Start on the host with `legendary launch`, which needs host access | Test: `import/legendary.rs` `games_start_through_legendary_on_the_host`; `src/permissions.rs` `only_games_we_start_on_the_host_need_host_spawn` |

## itch

| Claim | Covered by |
| --- | --- |
| Native or Flatpak | Test: `import/itch.rs` `native_and_flatpak_itch_import_the_same_games` |
| Games installed through the itch app | Test: `import/itch.rs` `soundtracks_are_left_out_and_orphaned_installs_reported`, `installs_still_in_the_write_ahead_log_are_imported` |
| With their playtime | Test: `import/itch.rs` `an_itch_game_carries_its_install_playtime_and_platform` |
| Start through the itch app | Test: `import/itch.rs` `itch_games_start_through_the_itch_app`, `a_game_installed_twice_starts_its_last_run_install`. Hardware: Game from another launcher |

## Desktop entries and Flatpaks

| Claim | Covered by |
| --- | --- |
| Games installed as apps | Test: `import/desktop.rs` `games_come_from_every_application_folder`, `non_games_hidden_entries_and_links_are_skipped`, `launchers_and_their_game_shortcuts_are_skipped` |
| Start on the host, which needs host access | Test: `import/desktop.rs` `a_game_keeps_its_exec_line_for_host_spawn`, `flatpak_games_run_through_flatpak`; `src/launch.rs` `desktop_entries_and_host_commands_spawn_their_argv_on_the_host`. Hardware: Game from another launcher |

## Added by hand

| Claim | Covered by |
| --- | --- |
| Native programs, and Windows games or installers | Test: `src/manual.rs` `adding_a_game_stores_it_with_its_command`, `a_windows_game_runs_through_proton_in_a_prefix_of_its_own`, `an_installer_gets_a_new_prefix_in_the_prefixes_folder`; `src/add_game.rs` `the_route_says_which_files_the_picker_takes_and_whether_the_game_runs_on_proton` |
| Native ones in the Steam Linux Runtime in the sandbox, or on the host if you choose | Test: `src/manual.rs` `a_native_game_runs_on_the_host_only_when_the_user_chooses_it`; `src/launch.rs` `runtime_native_games_run_in_the_steam_linux_runtime_through_umu`; `src/permissions.rs` `only_games_we_start_on_the_host_need_host_spawn` |
| Windows ones with Proton in the sandbox | Test: `src/launch.rs` `proton_games_run_through_umu_with_their_launch_config`. Hardware: Game added by hand |

## Every source

| Claim | Covered by |
| --- | --- |
| The importers only read each launcher's data | Test: `import/itch.rs` `the_live_database_is_left_untouched`; `import/lutris.rs` `the_live_database_is_copied_not_opened`. Hand check: read the importers in `src/sources/`; none writes, creates or removes a file in a launcher's folder, and `sqlite_copy.rs` copies a database to the temporary folder before opening it |
| And never its credentials | Test: `import/heroic.rs` `credential_files_are_never_opened`; `import/legendary.rs` `the_credentials_file_is_never_opened`; `import/steam.rs` `steams_login_files_are_never_opened`. Hand check: the files the Lutris (`pga.db`, art), Bottles (`library.yml`, `data.yml`, art) and Faugus (`games.json`, art) importers read hold no credentials |
| From the itch app's database they read its installs and games, never the profiles that hold the itch.io key | Test: `import/itch.rs` `the_profiles_that_hold_the_itch_key_are_never_read` |
| Preferences → Sources shows each source's folder, its last sync and that sync's problems | Test: `src/sources/location.rs` `resolving_records_the_location_in_source_state`; `src/sources/sync.rs` `syncing_a_source_imports_it_and_records_its_state`, `a_source_that_cannot_be_read_keeps_its_games_and_reports_why`. Hand check: the UI harness's Preferences → Sources shows Faugus's folder and its Last Synced row. Bug: the Desktop source says it is not found while it imports games (#852) |
| Copy Order picks the installed copy from the source highest in the list, native ahead of Windows, unless you chose a copy | Test: `src/game_groups/tests.rs` `the_default_copy_is_an_installed_one`, `the_default_copy_is_linux_then_unknown_then_windows`, `the_default_copy_follows_the_source_order`, `the_users_choice_is_the_preferred_copy`; `src/library.rs` `a_group_with_no_preferred_copy_starts_the_copy_the_source_order_puts_first` |
| Move Up and Move Down change the order | Test: `src/game_groups/tests.rs` `moving_a_source_swaps_it_with_its_neighbour_and_stops_at_either_end`. Hand check: `pindu-ui/src/sources.rs` `move_source` saves the moved order to the setting that `library_service.rs` reads for Play |

## Details and categories

| Claim | Covered by |
| --- | --- |
| A game's page shows the details its launcher keeps offline | Test: `src/sources/sync.rs` `a_sync_changes_only_the_sources_metadata`, `the_sources_release_year_and_controller_support_become_its_metadata`. Hand check: `pindu-ui/src/game_page.rs` shows the developer, categories, release year, controller support and description rows when they have a value |
| Heroic gives the developer, categories and description | Test: `import/heroic.rs` `metadata_comes_from_game_info`, `store_genres_become_categories_through_the_heroic_aliases` |
| Steam's app cache gives the developer, release year, categories and full or partial controller support | Test: `import/steam.rs` `non_games_are_filtered_and_platform_and_developer_come_from_appinfo`, `release_year_genres_and_controller_support_come_from_appinfo_in_both_layouts` |
| Lutris and Heroic's Amazon library give the release year | Test: `import/lutris.rs` `a_game_takes_its_service_runner_and_year_in_both_layouts`; `import/heroic.rs` `amazon_release_year_comes_from_its_release_date_in_both_layouts` |
| All categories come from one set based on Steam's, the fourteen the README names | Test: `src/category.rs` `steams_genre_ids_map_to_steams_names`, `every_category_is_one_a_provider_can_give`, `the_data_file_names_only_categories_of_the_set_and_each_id_once`. Hand check: `CATEGORIES` in `src/category.rs` lists the fourteen in the README's order |
| Heroic's and Wikidata's genres map onto the set and add none of their own | Test: `src/category.rs` `heroics_store_genres_map_through_aliases_ignoring_case`, `a_wikidata_genre_maps_by_its_qid`, `an_unmapped_genre_adds_no_category`; `src/wikidata.rs` `an_items_genres_become_categories_whatever_language_their_labels_are_in` |
| Categories typed in Edit… can be anything | Test: `src/metadata.rs` `typed_details_are_trimmed_with_genres_split_at_commas`, `a_providers_genres_add_to_the_sources_and_the_users_replace_both` |

## Folders the Flatpak cannot read

| Claim | Covered by |
| --- | --- |
| The Flatpak has no permission for Legendary's or the itch app's folders and does not ask for one | Test: `src/sources/location.rs` `the_sandbox_hides_exactly_the_sources_the_release_manifest_does_not_grant`; `import/legendary.rs` `the_sandbox_hides_legendary_so_its_folder_is_chosen_by_hand` |
| Preferences → Sources says so | Hand check: `pindu-ui/src/preferences.rs` `source_row` says "The sandbox cannot see this folder: choose it to import its games" and opens the row when the app runs in Flatpak, the source is not found and `sandbox_hides` names it. The UI harness runs outside Flatpak, so it cannot show this |
| **Choose…** imports a folder you pick | Test: `import/legendary.rs` `a_folder_chosen_through_the_portal_imports_the_same_games`; `src/sources/location.rs` `a_valid_override_wins_over_every_candidate`, `the_stored_override_is_used_and_kept` |
| Native packages find these folders themselves | Test: `import/legendary.rs` `native_legendary_is_found_in_the_config_folder`; `import/itch.rs` `native_and_flatpak_itch_import_the_same_games` |
