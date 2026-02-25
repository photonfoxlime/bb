# i18n wiring design

## 1. Where locale lives

- **AppState**: add `locale: String`. Always holds a supported locale (e.g. `"en-US"`, `"zh-CN"`, `"ja"`).
- **Initialization**: In `AppState::load()`, set `locale = crate::i18n::resolved_locale()`. Resolution order: (1) optional persisted locale in `app.toml` (`load_persisted_locale`), (2) environment (`LANG` / `LC_ALL` via `locale_from_env`), (3) `DEFAULT_LOCALE`. The result is normalized to a supported locale via `resolve_locale`.
- **Persistence**: Optional locale is stored in `<config_dir>/app.toml` (see `paths::AppPaths::app_config`). Use `save_locale` when the user changes locale (e.g. from settings); pass `None` to clear and fall back to env/default.

## 2. When the current locale is set for `t!(...)`

- **Single place**: At the very start of `AppState::view()` in `app.rs`, call `crate::i18n::set_app_locale(&self.locale)`. Then all code that runs during this view (document, settings, instruction panel, error banner) uses the same locale for the rest of the frame. No need to pass locale into subviews or call `set_app_locale` again.

## 3. Replacing UI strings

- **Document view** (`document.rs`): Replace every user-facing literal (buttons, placeholders, panel titles, status chip text, tooltips) with `rust_i18n::t!("key")` or `t!("key", var = value)` after `use rust_i18n::t`. All views already have access to `state`; locale is set once at the top of `view()` in `app.rs`.
- **Settings view** (`settings.rs`): Same: use `t!("key")` for "Settings", "Active", "Set Active", "Add", "Save", "Saved", "Delete this provider", labels, placeholders, etc.
- **Instruction panel** (`instruction_panel.rs`): Same: "Enter instruction...", "Inquire", "Inquiring...", "Expand", "Reduce", "Response", "Apply as Rewrite", "Append to Block", "Add as Child", "Dismiss".
- **Error banner** (`error_banner.rs`): Today it uses `prefix: &'static str` ("Error" / "Recovery mode") and `title()` builds a string. Change to:
  - Store a prefix key: e.g. `prefix_key: &'static str` (`"error"` or `"error_recovery_mode"`).
  - `title()` uses `t!(self.prefix_key)` and for the full title uses `t!("error_title_single", prefix = t!(...), message = self.latest.message)` and `t!("error_title_multi", ...)` (or a single key with vars). Use rust-i18n format args so translators can reorder. So: `error_title_single = "{prefix}: {message}"`, `error_title_multi = "{prefix} ({total} total): {message}"`, and in code pass `prefix`, `message`, `total`. Similarly "Earlier: {message}" → `t!("error_earlier", message = entry.message)`, "...and N older error(s)" → `t!("error_older_count", count = n)`.
- **Window title** (`main.rs`): Leave as literal `"Blooming Blockery"` for now (window title is set once at startup before any view runs). Optional later: set locale once at startup from env and use `t!("app_title")` if the API allows dynamic title.
- **File dialog titles** (`mount_file.rs`): "Save block to file" / "Load block from file" → `t!("save_block_to_file")` / `t!("load_block_from_file")` (locale already set when these run from a user action).

## 4. Translation keys and YAML

- Use flat, snake_case keys in `locales/en-US.yml`, `locales/zh-CN.yml`, `locales/ja.yml`.
- Namespace by screen/area only if we want (e.g. `doc_rewrite`, `settings_save`) or flat (`ui_dismiss`, `ui_friends`, `error`, `error_recovery_mode`, `error_earlier`, `error_older_count`, `error_title_single`, `error_title_multi`). Prefer flat for simplicity.
- Keys with variables use rust-i18n `%{var}` syntax in YAML and `t!("key", var = value)` in code.

## 5. Tests

- Any test that builds `AppState` must set `locale` (e.g. `locale: crate::i18n::DEFAULT_LOCALE.to_string()` or `"en-US".to_string()`).
- Tests that assert on `ErrorBanner::title()` or other localized strings: call `rust_i18n::set_locale("en-US")` at the start of the test so assertions on English text remain valid.

## 6. Summary of code touch points

| File              | Change |
|-------------------|--------|
| `app.rs`          | Add `locale: String` to `AppState`; in `load()` set from `i18n::resolved_locale()`; at start of `view()` call `set_app_locale(&self.locale)`. When user changes locale, call `i18n::save_locale(Some(&new_locale))` and update `state.locale`. |
| `document.rs`     | `use rust_i18n::t;` and replace all UI literals with `t!("key")` / `t!("key", var = value)`. |
| `instruction_panel.rs` | Same. |
| `settings.rs`     | Same. |
| `error_banner.rs` | Store `prefix_key`; `title()` and call sites use `t!(...)` with format args; document view uses `t!("error_earlier", message = ...)` and `t!("error_older_count", count = ...)`. |
| `mount_file.rs`   | Use `t!("save_block_to_file")` / `t!("load_block_from_file")` for dialog titles. |
| `main.rs`         | No change (or optional early set_app_locale + t!("app_title") if desired). |
| `locales/*.yml`   | Add all keys with en-US, zh-CN, ja translations. |
| Tests (multiple)  | Add `locale` to `AppState`; in tests that check banner title or other UI text, `set_locale("en-US")` at start. |
