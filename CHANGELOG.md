# Changelog

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
