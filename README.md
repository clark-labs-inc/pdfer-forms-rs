# pdfer_forms

Pure Rust AcroForm inspection and filling with a compatibility surface modeled after the PDF form-manipulation pieces of **pypdf** and **PyPDF2**.

This crate is deliberately focused on the form APIs people usually reach for when they are porting Python workflows:

- `get_fields`
- `get_form_text_fields`
- `get_pages_showing_field`
- `add_form_topname`
- `rename_form_topname`
- `set_need_appearances_writer`
- `update_page_form_field_values`
- `reattach_fields`
- `remove_annotations`
- PyPDF2-style camelCase aliases such as `updatePageFormFieldValues`

Under the hood it uses `lopdf`, which is itself a pure-Rust PDF library.

## Status

The crate is intended as a **native Rust port of the form-manipulation surface**, not as a line-for-line port of the full Python libraries.

Implemented here:

- AcroForm tree inspection
- qualified and partial field names
- text field value extraction
- page lookup for repeated widgets
- top-level form grouping / renaming
- `/NeedAppearances` control
- page-scoped field filling
- text/choice appearance regeneration
- button state updates for checkboxes / radios
- orphan widget reattachment to `/AcroForm /Fields`
- annotation removal for post-flatten cleanup
- optional flatten step that draws widget appearance streams into page content
- `FieldInput::KeepCurrent` for flattening without changing the stored value

Known caveats:


- Generated text appearances use built-in Type1 fonts and a simple WinAnsi text stream. The stored field value uses UTF-16BE, but generated appearance content itself is safest for ASCII / WinAnsi text.
- Signature-field appearance generation is not implemented.
- The API shape is intentionally close to pypdf / PyPDF2, but it remains idiomatic Rust rather than trying to mimic Python objects exactly.

## Rust version

This crate is configured for `lopdf = 0.40`, which currently expects Rust `1.85+`.

## Quick start

```rust,no_run
use pdfer_forms::{FieldInput, PageSelection, PdfReaderCompat, PdfWriterCompat};
use std::collections::BTreeMap;

fn main() -> pdfer_forms::Result<()> {
    let reader = PdfReaderCompat::load("input.pdf")?;
    let fields = reader.get_fields()?;
    println!("fields: {fields:#?}");

    let mut writer = PdfWriterCompat::from_reader(&reader);

    let mut updates = BTreeMap::new();
    updates.insert("sender.city".to_string(), FieldInput::from("Paris"));
    updates.insert(
        "sender.name".to_string(),
        FieldInput::from(("Alice Example", "/Helv", 11.0)),
    );

    writer.update_page_form_field_values(
        PageSelection::All,
        &updates,
        0,
        Some(false),
        false,
    )?;

    writer.save("output.pdf")?;
    Ok(())
}
```

## API notes

### Reader-like surface

```rust,ignore
use pdfer_forms::PdfReaderCompat;

let mut reader = PdfReaderCompat::load("form.pdf")?;
let all_fields = reader.get_fields()?;
let text_fields = reader.get_form_text_fields(false)?;
let pages = reader.get_pages_showing_field("sender.city")?;
reader.add_form_topname("form1")?;
reader.rename_form_topname("renamed_form")?;
```

### Writer-like surface

```rust,ignore
use pdfer_forms::{FieldInput, PageSelection, PdfWriterCompat};
use std::collections::BTreeMap;

let mut writer = PdfWriterCompat::load("form.pdf")?;
writer.set_need_appearances_writer(false)?;

let mut fields = BTreeMap::new();
fields.insert("check1".into(), FieldInput::from("/Yes"));
fields.insert("city".into(), FieldInput::from("Berlin"));
fields.insert("choices".into(), FieldInput::from(vec!["A".into(), "C".into()]));

writer.update_page_form_field_values(
    PageSelection::Index(0),
    &fields,
    0,
    Some(false),
    true,
)?;

writer.remove_annotations(Some(&["/Widget"]))?;
writer.save("flattened.pdf")?;
```

### PyPDF2 compatibility shims

```rust,ignore
use pdfer_forms::{PageSelection, PdfWriterCompat};
use std::collections::BTreeMap;

let mut writer = PdfWriterCompat::load("form.pdf")?;
writer.setNeedAppearancesWriter()?;

let mut fields = BTreeMap::new();
fields.insert("city".to_string(), "Berlin".to_string());
writer.updatePageFormFieldValues(PageSelection::Index(0), &fields, 0)?;
```

## Main types

- `PdfReaderCompat`
- `PdfWriterCompat`
- `FormField`
- `FieldValue`
- `FieldInput`
- `PageSelection`
- `PageHandle`
- `FieldSpecifier`

## Benchmarks — pdfer_forms vs pypdf / PyPDF2

Benchmarked against **pypdf 6.9.2** and **PyPDF2 3.0.1** on 9 real-world government
PDF forms (IRS, USCIS, GSA, Hong Kong IRD, Guatemala SAT) in English, Spanish,
and Chinese.

### Accuracy

| Metric | Result |
|---|---|
| Field name match rate | 1004/1011 (99.3%) |
| Field type match rate | 1004/1004 (100.0%) |
| Field value match rate | 1004/1004 (100.0%) |

The 7 name mismatches are encoding differences on a single Spanish-language PDF
where pypdf decodes non-ASCII field names (e.g. `DÍA`) while pdfer_forms
currently returns the raw bytes.

### Performance (average across 9 PDFs)

| Operation | pypdf | pdfer_forms | Speedup |
|---|---|---|---|
| `get_fields` | 12.1 ms | 0.51 ms | **23.6x faster** |
| `get_pages_showing_field` | 2.2 ms | 0.47 ms | **4.8x faster** |
| `fill_form` | 40.3 ms | 11.0 ms | **3.7x faster** |
| `remove_annotations` | 26.4 ms | 6.8 ms | **3.9x faster** |
| `get_form_text_fields` | 1.2 ms | 0.49 ms | **2.4x faster** |
| `load` | 1.9 ms | 5.2 ms | 2.7x slower\* |

\*Load is slower because `lopdf` eagerly parses the full cross-reference table;
pypdf uses lazy loading. For most workflows the total round-trip is still faster.

### API Parity

All 6 core pypdf form APIs pass on every test PDF. PyPDF2-style camelCase
aliases (`getFields`, `updatePageFormFieldValues`, etc.) are included.

## License

MIT OR Apache-2.0
