# Changelog

## 0.3.4

### Fixed
- Filling a merged field/widget no longer overwrites its non-terminal parent
  and creates a self-referential form tree.
- Radio groups retain their canonical parent value regardless of widget order,
  while each widget selects its own matching appearance state.
- Flattening multiple widgets on one page uses distinct XObject resource names.
- Checkbox/radio states and page lookup now follow direct or indirect `/Kids`
  arrays, and field reattachment no longer duplicates canonical descendants.

## 0.3.3

### Fixed
- Checkbox and radio-button updates now resolve common boolean-like inputs such
  as `on`, `checked`, and `true` to the widget's real non-Off appearance state
  instead of silently writing `/Off`.

### Added
- Public `resolve_button_state` API for callers that need the same exact-state
  and boolean-alias resolution used by the form writer.

## 0.3.1

### Added
- `update_page_form_field_values` now also matches a requested field by its leaf
  `/T` (the final `.`-segment) when the fully-qualified name and partial name
  don't match. Callers (LLM agents) reliably copy the leaf field name but often
  drop/mangle intermediate AcroForm group segments (e.g. request
  `…Page1[0].f1_07[0]` for a field actually at `…Page1[0].Address_ReadOrder[0].f1_07[0]`);
  the leaf `/T` identifies the widget, so these now resolve instead of silently
  not matching.


## 0.3.0

### Fixed
- `update_page_form_field_values` now fills *merged field+widget* annotations
  (the annotation itself carries `/FT` and `/T`, as in most IRS / USCIS / DMV
  forms) by their **fully-qualified name** — the same name `get_fields()`
  returns. Previously the qualified name was computed from the widget's
  `/Parent`, dropping the leaf `/T` segment, so filling by the name reported by
  inspection silently matched nothing.

### Changed (breaking)
- `update_page_form_field_values` now returns `Result<FillReport>` instead of
  `Result<()>`. `FillReport` reports `matched` / `unmatched` requested field
  names and `widgets_updated`, so callers can detect (and surface) a fill that
  matched no widget instead of treating a no-op as success.
