use pdfer_forms::{FieldInput, FieldValue, FormField, PageSelection, PdfReaderCompat, PdfWriterCompat};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Serialize)]
struct BenchmarkResults {
    library: String,
    version: String,
    pdfs: BTreeMap<String, PdfResult>,
}

#[derive(Serialize)]
struct PdfResult {
    path: String,
    timings_ms: BTreeMap<String, f64>,
    field_count: usize,
    text_field_count: usize,
    pages: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    fields: Option<BTreeMap<String, FieldInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text_fields: Option<BTreeMap<String, Option<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pages_for_first_field: Option<Vec<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fill_input: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fill_readback: Option<BTreeMap<String, String>>,
    errors: Vec<String>,
}

#[derive(Serialize)]
struct FieldInfo {
    field_type: Option<String>,
    value: Option<String>,
    default_value: Option<String>,
    flags: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    states: Option<Vec<String>>,
}

fn field_value_to_string(fv: &FieldValue) -> String {
    match fv {
        FieldValue::Text(s) => s.clone(),
        FieldValue::Name(s) => s.clone(),
        FieldValue::List(items) => items.join(", "),
        FieldValue::Null => String::new(),
    }
}

fn serialize_field(field: &FormField) -> FieldInfo {
    FieldInfo {
        field_type: field.field_type.as_ref().map(|ft| format!("/{ft}")),
        value: field.value.as_ref().map(field_value_to_string),
        default_value: field.default_value.as_ref().map(field_value_to_string),
        flags: field.flags,
        states: if field.states.is_empty() {
            None
        } else {
            Some(field.states.clone())
        },
    }
}

fn collect_pdfs(base: &Path) -> Vec<PathBuf> {
    let mut pdfs = Vec::new();
    for lang in &["en", "es", "zh"] {
        let dir = base.join(lang);
        if dir.is_dir() {
            let mut entries: Vec<_> = std::fs::read_dir(&dir)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|ext| ext == "pdf"))
                .collect();
            entries.sort();
            pdfs.extend(entries);
        }
    }
    pdfs
}

fn rel_path(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn bench_pdf(pdf_path: &Path, pdf_dir: &Path) -> PdfResult {
    let rel = rel_path(pdf_path, pdf_dir);
    let mut result = PdfResult {
        path: rel.clone(),
        timings_ms: BTreeMap::new(),
        field_count: 0,
        text_field_count: 0,
        pages: 0,
        fields: None,
        text_fields: None,
        pages_for_first_field: None,
        fill_input: None,
        fill_readback: None,
        errors: Vec::new(),
    };

    // 1. Load
    let start = Instant::now();
    let reader = match PdfReaderCompat::load(pdf_path) {
        Ok(r) => r,
        Err(e) => {
            result.errors.push(format!("load: {e}"));
            return result;
        }
    };
    result.timings_ms.insert("load".into(), start.elapsed().as_secs_f64() * 1000.0);
    result.pages = reader.pages().len();

    // 2. get_fields
    let start = Instant::now();
    let fields = match reader.get_fields() {
        Ok(Some(f)) => f,
        Ok(None) => {
            result.timings_ms.insert("get_fields".into(), start.elapsed().as_secs_f64() * 1000.0);
            result.errors.push("no AcroForm fields".into());
            return result;
        }
        Err(e) => {
            result.timings_ms.insert("get_fields".into(), start.elapsed().as_secs_f64() * 1000.0);
            result.errors.push(format!("get_fields: {e}"));
            return result;
        }
    };
    result.timings_ms.insert("get_fields".into(), start.elapsed().as_secs_f64() * 1000.0);
    result.field_count = fields.len();

    // Serialize fields
    let mut field_map = BTreeMap::new();
    for (name, field) in &fields {
        field_map.insert(name.clone(), serialize_field(field));
    }
    result.fields = Some(field_map);

    // 3. get_form_text_fields
    let start = Instant::now();
    match reader.get_form_text_fields(true) {
        Ok(tf) => {
            result.timings_ms.insert("get_form_text_fields".into(), start.elapsed().as_secs_f64() * 1000.0);
            result.text_field_count = tf.len();
            result.text_fields = Some(tf);
        }
        Err(e) => {
            result.timings_ms.insert("get_form_text_fields".into(), start.elapsed().as_secs_f64() * 1000.0);
            result.errors.push(format!("get_form_text_fields: {e}"));
        }
    }

    // 4. get_pages_showing_field (use first text field)
    let first_text_field = fields.iter().find(|(_, f)| f.is_text_field()).map(|(n, _)| n.clone());
    if let Some(ref field_name) = first_text_field {
        let start = Instant::now();
        match reader.get_pages_showing_field(field_name.as_str()) {
            Ok(pages) => {
                result.timings_ms.insert("get_pages_showing_field".into(), start.elapsed().as_secs_f64() * 1000.0);
                result.pages_for_first_field = Some(pages.iter().map(|p| p.index).collect());
            }
            Err(e) => {
                result.timings_ms.insert("get_pages_showing_field".into(), start.elapsed().as_secs_f64() * 1000.0);
                result.errors.push(format!("get_pages_showing_field: {e}"));
            }
        }
    }

    // 5. Fill form (first 3 text fields)
    let mut fill_data = BTreeMap::new();
    let mut count = 0;
    for (name, field) in &fields {
        if field.is_text_field() && count < 3 {
            fill_data.insert(name.clone(), FieldInput::from(format!("TEST_{count}")));
            count += 1;
        }
    }
    if !fill_data.is_empty() {
        let fill_input: BTreeMap<String, String> = fill_data
            .iter()
            .map(|(k, _)| {
                let idx = k.as_str();
                // We know we inserted FieldInput::from(format!("TEST_{count}"))
                // so reconstruct the expected value
                (k.clone(), format!("TEST_{}", fill_data.keys().position(|x| x == idx).unwrap()))
            })
            .collect();

        let start = Instant::now();
        let fill_result = (|| -> pdfer_forms::Result<BTreeMap<String, String>> {
            let mut writer = PdfWriterCompat::load(pdf_path)?;
            writer.set_need_appearances_writer(false)?;
            writer.update_page_form_field_values(PageSelection::All, &fill_data, 0, Some(false), false)?;

            let tmp = tempfile::NamedTempFile::new().map_err(|e| {
                pdfer_forms::PdferError::Message(format!("tmpfile: {e}"))
            })?;
            let tmp_path = tmp.path().to_path_buf();
            writer.save(&tmp_path)?;

            // Read back
            let verify = PdfReaderCompat::load(&tmp_path)?;
            let verify_fields = verify.get_fields()?.unwrap_or_default();
            let mut readback = BTreeMap::new();
            for (name, field) in &verify_fields {
                if let Some(val) = &field.value {
                    let s = field_value_to_string(val);
                    if !s.is_empty() {
                        readback.insert(name.clone(), s);
                    }
                }
            }
            Ok(readback)
        })();

        result.timings_ms.insert("fill_form".into(), start.elapsed().as_secs_f64() * 1000.0);
        result.fill_input = Some(fill_input);
        match fill_result {
            Ok(readback) => result.fill_readback = Some(readback),
            Err(e) => result.errors.push(format!("fill_form: {e}")),
        }
    }

    // 6. Remove annotations
    let start = Instant::now();
    let ra_result = (|| -> pdfer_forms::Result<()> {
        let mut writer = PdfWriterCompat::load(pdf_path)?;
        writer.remove_annotations(Some(&["/Widget"]))?;
        let tmp = tempfile::NamedTempFile::new().map_err(|e| {
            pdfer_forms::PdferError::Message(format!("tmpfile: {e}"))
        })?;
        writer.save(tmp.path())?;
        Ok(())
    })();
    result.timings_ms.insert("remove_annotations".into(), start.elapsed().as_secs_f64() * 1000.0);
    if let Err(e) = ra_result {
        result.errors.push(format!("remove_annotations: {e}"));
    }

    result
}

fn main() {
    let bench_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let pdf_dir = bench_dir.join("pdfs");

    let pdfs = collect_pdfs(&pdf_dir);
    eprintln!("pdfer_forms benchmark");
    eprintln!("PDFs found: {}", pdfs.len());

    let mut results = BenchmarkResults {
        library: "pdfer_forms".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        pdfs: BTreeMap::new(),
    };

    for pdf_path in &pdfs {
        let rel = rel_path(pdf_path, &pdf_dir);
        eprint!("  [pdfer_forms] {rel} ... ");
        let pdf_result = bench_pdf(pdf_path, &pdf_dir);
        eprintln!(
            "{} fields, load={:.1}ms, get_fields={:.1}ms{}",
            pdf_result.field_count,
            pdf_result.timings_ms.get("load").unwrap_or(&0.0),
            pdf_result.timings_ms.get("get_fields").unwrap_or(&0.0),
            if pdf_result.errors.is_empty() {
                String::new()
            } else {
                format!(" [errors: {}]", pdf_result.errors.join("; "))
            }
        );
        results.pdfs.insert(rel, pdf_result);
    }

    let json = serde_json::to_string_pretty(&results).unwrap();
    let output_path = bench_dir.join("rust_benchmark.json");
    std::fs::write(&output_path, &json).unwrap();
    eprintln!("\nResults written to {}", output_path.display());

    // Also print to stdout for piping
    println!("{json}");
}
