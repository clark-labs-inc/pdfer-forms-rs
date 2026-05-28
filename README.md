# pdfer_forms — Fast Pure-Rust PDF Forms & Document Operations

[![Crates.io](https://img.shields.io/crates/v/pdfer_forms.svg)](https://crates.io/crates/pdfer_forms)
[![docs.rs](https://img.shields.io/docsrs/pdfer_forms)](https://docs.rs/pdfer_forms)
[![Crates.io downloads](https://img.shields.io/crates/d/pdfer_forms.svg)](https://crates.io/crates/pdfer_forms)
[![License](https://img.shields.io/crates/l/pdfer_forms.svg)](https://github.com/clark-labs-inc/pdfer-forms-rs#license)

**`pdfer_forms`** is a fast, pure-Rust library for **filling PDF forms**, **inspecting AcroForm fields**, **flattening fillable PDFs**, and **document operations** (merge, split, rotate, encrypt/decrypt) — with an API modeled on Python's [`pypdf`](https://pypdf.readthedocs.io/) and `PyPDF2`.

Built and maintained by **Stanislav Kirdey, [Clark Labs Inc.](https://github.com/clark-labs-inc)**

- **Pure Rust**, no Python, no C dependencies (built on [`lopdf`](https://crates.io/crates/lopdf))
- **Fast** — up to **23× faster than pypdf** on real-world government forms (see benchmarks below)
- **PyPDF / PyPDF2 compatibility layer** — familiar `get_fields`, `update_page_form_field_values`, `updatePageFormFieldValues`, etc.
- **AcroForm inspection, form filling, and flattening** in one crate
- **Document operations** — merge, split, rotate, encrypt, decrypt (replaces `qpdf` CLI)
- `#![forbid(unsafe_code)]`

## Why pdfer_forms?

If you are porting a Python PDF workflow to Rust, or building a Rust service that needs to fill fillable PDFs (government forms, tax forms, application forms, contracts), the pure-Rust PDF ecosystem has historically been thin on AcroForm support. `pdfer_forms` closes that gap:

- Inspect every AcroForm field, including qualified and partial names
- Fill text, checkbox, radio, and choice (listbox / combo) fields
- Regenerate widget appearance streams so filled values render in every viewer
- Flatten forms (draw appearances into page content) for archival or print
- Strip widget annotations for final delivery
- Reattach orphan widgets to `/AcroForm /Fields`
- Toggle `/NeedAppearances`, group fields under a top-level name, and more

## Install

```toml
[dependencies]
pdfer_forms = "0.2"
```

Requires Rust **1.85+** (set by the pinned `lopdf = 0.40` dependency).

## Quick start — fill a PDF form in Rust

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

## Document operations — merge, split, rotate, encrypt

New in **0.2.0**: the `ops` module provides pure-Rust replacements for `qpdf` / `pdftk` CLI operations.

```rust,no_run
use pdfer_forms::ops;

fn main() -> pdfer_forms::Result<()> {
    // Merge multiple PDFs
    let mut merged = ops::merge_files(&["doc1.pdf", "doc2.pdf", "doc3.pdf"])?;
    merged.save("merged.pdf")?;

    // Split: extract pages 1 and 3
    let doc = lopdf::Document::load("input.pdf")?;
    let mut subset = ops::split_pages(&doc, &[1, 3])?;
    subset.save("pages_1_3.pdf")?;

    // Split into one PDF per page
    let mut pages = ops::split_each_page(&doc)?;
    for (i, page_doc) in pages.iter_mut().enumerate() {
        page_doc.save(format!("page_{}.pdf", i + 1))?;
    }

    // Rotate pages 90° clockwise
    let mut doc = lopdf::Document::load("input.pdf")?;
    ops::rotate_pages(&mut doc, &[1, 2], 90)?;
    doc.save("rotated.pdf")?;

    // Encrypt with passwords
    let mut doc = lopdf::Document::load("input.pdf")?;
    ops::encrypt_document(&mut doc, "user_pass", "owner_pass")?;
    doc.save("encrypted.pdf")?;

    // Decrypt
    let mut doc = lopdf::Document::load("encrypted.pdf")?;
    ops::decrypt_document(&mut doc, "user_pass")?;
    doc.save("decrypted.pdf")?;

    Ok(())
}
```

### Available functions

| Function | Description |
|---|---|
| `ops::merge_documents(docs)` | Merge multiple `lopdf::Document`s into one |
| `ops::merge_files(paths)` | Load and merge PDFs from file paths |
| `ops::split_pages(doc, pages)` | Extract specific pages (1-based) into a new document |
| `ops::split_each_page(doc)` | Split into one document per page |
| `ops::rotate_pages(doc, pages, degrees)` | Rotate pages by 0/90/180/270 degrees |
| `ops::encrypt_document(doc, user_pw, owner_pw)` | Encrypt with AES-128 |
| `ops::decrypt_document(doc, password)` | Decrypt with password |

## Features

- AcroForm tree inspection
- Qualified and partial field names
- Text field value extraction
- Page lookup for repeated widgets
- Top-level form grouping / renaming
- `/NeedAppearances` control
- Page-scoped field filling
- Text and choice appearance regeneration
- Button state updates for checkboxes and radio groups
- Orphan widget reattachment to `/AcroForm /Fields`
- Annotation removal for post-flatten cleanup
- Optional flatten step that draws widget appearance streams into page content
- `FieldInput::KeepCurrent` for flattening without changing the stored value

### Known caveats

- Generated text appearances use built-in Type1 fonts and a simple WinAnsi text stream. The stored field value uses UTF-16BE, but generated appearance content itself is safest for ASCII / WinAnsi text.
- Signature-field appearance generation is not implemented.
- The API is intentionally close to pypdf / PyPDF2, but remains idiomatic Rust rather than mimicking Python objects exactly.

## PyPDF / PyPDF2 API compatibility

`pdfer_forms` mirrors the form-manipulation surface of `pypdf` and `PyPDF2`, including camelCase aliases:

| pypdf / PyPDF2 | pdfer_forms |
|---|---|
| `PdfReader.get_fields()` | `PdfReaderCompat::get_fields` |
| `PdfReader.get_form_text_fields()` | `PdfReaderCompat::get_form_text_fields` |
| `PdfReader.get_pages_showing_field()` | `PdfReaderCompat::get_pages_showing_field` |
| `PdfWriter.add_form_topname()` | `PdfReaderCompat::add_form_topname` |
| `PdfWriter.rename_form_topname()` | `PdfReaderCompat::rename_form_topname` |
| `PdfWriter.set_need_appearances_writer()` | `PdfWriterCompat::set_need_appearances_writer` |
| `PdfWriter.update_page_form_field_values()` | `PdfWriterCompat::update_page_form_field_values` |
| `PdfWriter.reattach_fields()` | `PdfWriterCompat::reattach_fields` |
| `PdfWriter.remove_annotations()` | `PdfWriterCompat::remove_annotations` |
| `updatePageFormFieldValues` (PyPDF2) | `updatePageFormFieldValues` |
| `setNeedAppearancesWriter` (PyPDF2) | `setNeedAppearancesWriter` |

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

### PyPDF2 camelCase shims

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

- `PdfReaderCompat` — pypdf-style reader wrapper
- `PdfWriterCompat` — pypdf-style writer wrapper
- `FormField` — an AcroForm field with value, type, and widgets
- `FieldValue` — decoded field value (text, button state, choice list)
- `FieldInput` — input variant for field updates (text, button, choice, `KeepCurrent`)
- `PageSelection` — `All` or `Index(n)` scope for updates
- `PageHandle` — page identity helper
- `FieldSpecifier` — qualified / partial field name resolver

## Benchmarks — pdfer_forms vs pypdf / PyPDF2

Benchmarked against **pypdf 6.9.2** and **PyPDF2 3.0.1** on 9 real-world government PDF forms (IRS, USCIS, GSA, Hong Kong IRD, Guatemala SAT) in English, Spanish, and Chinese.

### Accuracy

| Metric | Result |
|---|---|
| Field name match rate | 1004/1011 (99.3%) |
| Field type match rate | 1004/1004 (100.0%) |
| Field value match rate | 1004/1004 (100.0%) |

The 7 name mismatches are encoding differences on a single Spanish-language PDF where pypdf decodes non-ASCII field names (e.g. `DÍA`) while pdfer_forms currently returns the raw bytes.

### Performance (average across 9 PDFs)

| Operation | pypdf | pdfer_forms | Speedup |
|---|---|---|---|
| `get_fields` | 12.1 ms | 0.51 ms | **23.6× faster** |
| `get_pages_showing_field` | 2.2 ms | 0.47 ms | **4.8× faster** |
| `fill_form` | 40.3 ms | 11.0 ms | **3.7× faster** |
| `remove_annotations` | 26.4 ms | 6.8 ms | **3.9× faster** |
| `get_form_text_fields` | 1.2 ms | 0.49 ms | **2.4× faster** |
| `load` | 1.9 ms | 5.2 ms | 2.7× slower\* |

\*Load is slower because `lopdf` eagerly parses the full cross-reference table; pypdf uses lazy loading. For most workflows the total round-trip is still faster.

### API parity

All 6 core pypdf form APIs pass on every test PDF. PyPDF2-style camelCase aliases (`getFields`, `updatePageFormFieldValues`, etc.) are included.

## Related crates

- [`lopdf`](https://crates.io/crates/lopdf) — the pure-Rust PDF library this crate is built on
- [`printpdf`](https://crates.io/crates/printpdf) — for generating PDFs from scratch
- [`pdf`](https://crates.io/crates/pdf) — another pure-Rust PDF reader

## Contributing

Issues and pull requests are welcome at <https://github.com/clark-labs-inc/pdfer-forms-rs>.

## Citation

Citation authorship: Stanislav Kirdey, Clark Labs Inc. See [`CITATION.cff`](CITATION.cff) for machine-readable citation metadata.

## License

Licensed under either of

- Apache License, Version 2.0 (<https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license (<https://opensource.org/licenses/MIT>)

at your option.

---

© Stanislav Kirdey, Clark Labs Inc. • [www.clarkchat.com](https://www.clarkchat.com)
>
>`pdfer_forms` is not affiliated with the authors of pypdf or PyPDF2. pypdf and PyPDF2 are trademarks of their respective owners; compatibility is provided for porting convenience.
