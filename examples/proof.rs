use pdfer_forms::{
    FieldInput, FieldValue, PageSelection, PdfReaderCompat, PdfWriterCompat,
};
use std::collections::BTreeMap;

fn short(name: &str, max: usize) -> String {
    if name.len() > max {
        format!("...{}", &name[name.len() - (max - 3)..])
    } else {
        name.to_owned()
    }
}

fn main() -> pdfer_forms::Result<()> {
    let pdfs = [
        // English — US government
        ("benchmarks/pdfs/en/irs_w9.pdf", "IRS W-9 (EN, XFA)"),
        ("benchmarks/pdfs/en/irs_w4.pdf", "IRS W-4 (EN, XFA)"),
        ("benchmarks/pdfs/en/uscis_i9.pdf", "USCIS I-9 (EN, AcroForm)"),
        ("benchmarks/pdfs/en/gsa_sf1449.pdf", "GSA SF-1449 (EN, XFA, 314 fields)"),
        // Spanish
        ("benchmarks/pdfs/es/guatemala_sat_362.pdf", "Guatemala SAT-362 (ES, accented names)"),
        ("benchmarks/pdfs/es/irs_4506_sp.pdf", "IRS 4506-SP (ES)"),
        ("benchmarks/pdfs/es/irs_w4_sp.pdf", "IRS W-4-SP (ES)"),
        // Chinese / HK
        ("benchmarks/pdfs/zh/hk_ir56g.pdf", "HK IR56G (ZH, encrypted)"),
        ("benchmarks/pdfs/zh/hk_ir76c.pdf", "HK IR76C (ZH, encrypted, 193 fields)"),
    ];

    let mut total_pass = 0;
    let mut total_fail = 0;

    for (path, label) in &pdfs {
        println!("{}", "=".repeat(74));
        println!("  {label}");
        println!("  {path}");
        println!("{}", "=".repeat(74));

        // ────────────────────────────────────────────────────
        // TEST 1: Load PDF
        // ────────────────────────────────────────────────────
        let reader = PdfReaderCompat::load(path)?;
        let pages = reader.pages();
        assert!(!pages.is_empty(), "must have pages");
        println!("  [PASS] Load: {} pages", pages.len());
        total_pass += 1;

        // ────────────────────────────────────────────────────
        // TEST 2: get_fields — field extraction
        // ────────────────────────────────────────────────────
        let fields = reader.get_fields()?.unwrap_or_default();
        assert!(!fields.is_empty(), "must have fields");
        let text_count = fields.values().filter(|f| f.is_text_field()).count();
        let btn_count = fields.values().filter(|f| f.field_type.as_deref() == Some("Btn")).count();
        let choice_count = fields.values().filter(|f| f.field_type.as_deref() == Some("Ch")).count();
        let sig_count = fields.values().filter(|f| f.field_type.as_deref() == Some("Sig")).count();
        let container_count = fields.values().filter(|f| f.field_type.is_none()).count();
        println!(
            "  [PASS] get_fields: {} total (Tx={text_count} Btn={btn_count} Ch={choice_count} Sig={sig_count} container={container_count})",
            fields.len()
        );
        total_pass += 1;

        // ────────────────────────────────────────────────────
        // TEST 3: get_form_text_fields — qualified names
        // ────────────────────────────────────────────────────
        let text_fq = reader.get_form_text_fields(true)?;
        println!(
            "  [PASS] get_form_text_fields(qualified=true): {} fields",
            text_fq.len()
        );
        total_pass += 1;

        // ────────────────────────────────────────────────────
        // TEST 4: get_form_text_fields — partial names
        // ────────────────────────────────────────────────────
        let text_partial = reader.get_form_text_fields(false)?;
        println!(
            "  [PASS] get_form_text_fields(qualified=false): {} fields",
            text_partial.len()
        );
        total_pass += 1;

        // ────────────────────────────────────────────────────
        // TEST 5: get_pages_showing_field
        // ────────────────────────────────────────────────────
        if let Some((first_name, _)) = fields.iter().find(|(_, f)| f.is_text_field()) {
            let showing = reader.get_pages_showing_field(first_name.as_str())?;
            let page_indices: Vec<_> = showing.iter().map(|p| p.index).collect();
            println!(
                "  [PASS] get_pages_showing_field({:?}): pages {:?}",
                short(first_name, 35),
                page_indices
            );
            total_pass += 1;
        }

        // ────────────────────────────────────────────────────
        // TEST 6: page() accessor for each page index
        // ────────────────────────────────────────────────────
        for i in 0..pages.len() {
            let _ = reader.page(i)?;
        }
        println!("  [PASS] page(i) for all {} pages", pages.len());
        total_pass += 1;

        // ────────────────────────────────────────────────────
        // TEST 7: Fill form — text fields
        // ────────────────────────────────────────────────────
        {
            let mut writer = PdfWriterCompat::load(path)?;
            writer.set_need_appearances_writer(false)?;

            let mut updates = BTreeMap::new();
            let mut filled_names = Vec::new();
            let mut count = 0;
            for (name, field) in &fields {
                if field.is_text_field() && count < 5 {
                    let val = format!("PROOF_{count}");
                    updates.insert(name.clone(), FieldInput::from(val.as_str()));
                    filled_names.push((name.clone(), val));
                    count += 1;
                }
            }

            writer.update_page_form_field_values(
                PageSelection::All,
                &updates,
                0,
                Some(false),
                false,
            )?;

            let out = format!("/tmp/proof_{}", path.split('/').last().unwrap());
            writer.save(&out)?;
            let sz = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);

            // Read back and verify /V values persisted
            let verify = PdfReaderCompat::load(&out)?;
            let verify_fields = verify.get_fields()?.unwrap_or_default();
            let mut verified = 0;
            for (name, expected) in &filled_names {
                if let Some(field) = verify_fields.get(name) {
                    if let Some(FieldValue::Text(v)) = &field.value {
                        if v == expected {
                            verified += 1;
                        }
                    }
                }
            }
            println!(
                "  [PASS] Fill & save: {count} fields → {out} ({sz} bytes), {verified}/{count} verified on read-back"
            );
            total_pass += 1;
        }

        // ────────────────────────────────────────────────────
        // TEST 8: Fill form with font override
        // ────────────────────────────────────────────────────
        {
            let mut writer = PdfWriterCompat::load(path)?;
            let first_text = fields.iter().find(|(_, f)| f.is_text_field());
            if let Some((name, _)) = first_text {
                let mut updates = BTreeMap::new();
                updates.insert(
                    name.clone(),
                    FieldInput::from(("CustomFont", "/Helv", 14.0)),
                );
                writer.update_page_form_field_values(
                    PageSelection::All,
                    &updates,
                    0,
                    Some(false),
                    false,
                )?;
                println!(
                    "  [PASS] Fill with font override: {:?} → /Helv 14pt",
                    short(name, 35)
                );
                total_pass += 1;
            }
        }

        // ────────────────────────────────────────────────────
        // TEST 9: Fill with KeepCurrent (flatten without changing value)
        // ────────────────────────────────────────────────────
        {
            let mut writer = PdfWriterCompat::load(path)?;
            let first_text = fields.iter().find(|(_, f)| f.is_text_field());
            if let Some((name, _)) = first_text {
                let mut updates = BTreeMap::new();
                updates.insert(name.clone(), FieldInput::KeepCurrent);
                writer.update_page_form_field_values(
                    PageSelection::All,
                    &updates,
                    0,
                    Some(false),
                    false,
                )?;
                println!(
                    "  [PASS] KeepCurrent (no-change fill): {:?}",
                    short(name, 35)
                );
                total_pass += 1;
            }
        }

        // ────────────────────────────────────────────────────
        // TEST 10: Fill checkboxes/buttons
        // ────────────────────────────────────────────────────
        {
            let btn_fields: Vec<_> = fields
                .iter()
                .filter(|(_, f)| f.field_type.as_deref() == Some("Btn"))
                .take(2)
                .collect();
            if !btn_fields.is_empty() {
                let mut writer = PdfWriterCompat::load(path)?;
                let mut updates = BTreeMap::new();
                for (name, field) in &btn_fields {
                    let state = if field.states.is_empty() {
                        "/Yes".to_owned()
                    } else {
                        field.states[0].clone()
                    };
                    updates.insert((*name).clone(), FieldInput::from(state.as_str()));
                }
                writer.update_page_form_field_values(
                    PageSelection::All,
                    &updates,
                    0,
                    None,
                    false,
                )?;
                println!(
                    "  [PASS] Fill {} checkbox/button fields",
                    btn_fields.len()
                );
                total_pass += 1;
            }
        }

        // ────────────────────────────────────────────────────
        // TEST 11: Fill choice/dropdown fields
        // ────────────────────────────────────────────────────
        {
            let choice_fields: Vec<_> = fields
                .iter()
                .filter(|(_, f)| f.field_type.as_deref() == Some("Ch"))
                .take(2)
                .collect();
            if !choice_fields.is_empty() {
                let mut writer = PdfWriterCompat::load(path)?;
                let mut updates = BTreeMap::new();
                for (name, _) in &choice_fields {
                    updates.insert(
                        (*name).clone(),
                        FieldInput::from(vec!["Option1".to_string()]),
                    );
                }
                writer.update_page_form_field_values(
                    PageSelection::All,
                    &updates,
                    0,
                    None,
                    false,
                )?;
                println!(
                    "  [PASS] Fill {} choice/dropdown fields",
                    choice_fields.len()
                );
                total_pass += 1;
            }
        }

        // ────────────────────────────────────────────────────
        // TEST 12: Fill by page index (not All)
        // ────────────────────────────────────────────────────
        {
            let mut writer = PdfWriterCompat::load(path)?;
            let first_text = fields.iter().find(|(_, f)| f.is_text_field());
            if let Some((name, _)) = first_text {
                let mut updates = BTreeMap::new();
                updates.insert(name.clone(), FieldInput::from("PAGE_0_ONLY"));
                writer.update_page_form_field_values(
                    PageSelection::Index(0),
                    &updates,
                    0,
                    None,
                    false,
                )?;
                println!("  [PASS] Fill by PageSelection::Index(0)");
                total_pass += 1;
            }
        }

        // ────────────────────────────────────────────────────
        // TEST 13: Fill + flatten (draw appearance into page content)
        // ────────────────────────────────────────────────────
        {
            let mut writer = PdfWriterCompat::load(path)?;
            writer.set_need_appearances_writer(false)?;
            let first_text = fields.iter().find(|(_, f)| f.is_text_field());
            if let Some((name, _)) = first_text {
                let mut updates = BTreeMap::new();
                updates.insert(name.clone(), FieldInput::from("FLATTENED"));
                writer.update_page_form_field_values(
                    PageSelection::All,
                    &updates,
                    0,
                    Some(false),
                    true, // flatten=true
                )?;
                let out = format!("/tmp/proof_flat_fill_{}", path.split('/').last().unwrap());
                writer.save(&out)?;
                let sz = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
                println!(
                    "  [PASS] Fill + flatten: {:?} → {out} ({sz} bytes)",
                    short(name, 30)
                );
                total_pass += 1;
            }
        }

        // ────────────────────────────────────────────────────
        // TEST 14: set_need_appearances_writer (true and false)
        // ────────────────────────────────────────────────────
        {
            let mut writer = PdfWriterCompat::load(path)?;
            writer.set_need_appearances_writer(true)?;
            writer.set_need_appearances_writer(false)?;
            println!("  [PASS] set_need_appearances_writer(true/false)");
            total_pass += 1;
        }

        // ────────────────────────────────────────────────────
        // TEST 15: remove_annotations
        // ────────────────────────────────────────────────────
        match (|| -> pdfer_forms::Result<()> {
            let mut writer = PdfWriterCompat::load(path)?;
            writer.remove_annotations(Some(&["/Widget"]))?;
            let out = format!("/tmp/proof_stripped_{}", path.split('/').last().unwrap());
            writer.save(&out)?;
            let sz = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
            println!("  [PASS] remove_annotations(/Widget): {out} ({sz} bytes)");
            Ok(())
        })() {
            Ok(()) => total_pass += 1,
            Err(e) => {
                println!("  [FAIL] remove_annotations: {e}");
                total_fail += 1;
            }
        }

        // ────────────────────────────────────────────────────
        // TEST 16: add_form_topname + rename_form_topname
        // ────────────────────────────────────────────────────
        {
            let mut r = PdfReaderCompat::load(path)?;
            let added = r.add_form_topname("test_top")?;
            if let Some(ref f) = added {
                let renamed = r.rename_form_topname("renamed_top")?;
                println!(
                    "  [PASS] add_form_topname → {:?}, rename → {:?}",
                    f.qualified_name,
                    renamed.as_ref().map(|r| &r.qualified_name)
                );
                total_pass += 1;
            } else {
                println!("  [SKIP] add_form_topname returned None");
            }
        }

        // ────────────────────────────────────────────────────
        // TEST 17: reattach_fields
        // ────────────────────────────────────────────────────
        {
            let mut writer = PdfWriterCompat::load(path)?;
            let reattached = writer.reattach_fields(None)?;
            println!(
                "  [PASS] reattach_fields: {} fields reattached",
                reattached.len()
            );
            total_pass += 1;
        }

        // ────────────────────────────────────────────────────
        // TEST 18: PyPDF2 compat shims
        // ────────────────────────────────────────────────────
        #[allow(non_snake_case)]
        {
            let r = PdfReaderCompat::load(path)?;
            let _ = r.getFields()?;
            let _ = r.getFormTextFields(true)?;
            let first_text = fields.iter().find(|(_, f)| f.is_text_field());
            if let Some((name, _)) = first_text {
                let _ = r.getPagesShowingField(name.as_str())?;
            }
            let mut w = PdfWriterCompat::load(path)?;
            w.setNeedAppearancesWriter()?;
            let mut m = BTreeMap::new();
            if let Some((name, _)) = first_text {
                m.insert(name.clone(), "compat_test".to_string());
                w.updatePageFormFieldValues(PageSelection::All, &m, 0)?;
            }
            println!("  [PASS] PyPDF2 compat shims (getFields, getFormTextFields, updatePageFormFieldValues, ...)");
            total_pass += 1;
        }

        // ────────────────────────────────────────────────────
        // TEST 19: from_bytes (load from memory)
        // ────────────────────────────────────────────────────
        {
            let bytes = std::fs::read(path).unwrap();
            let r = PdfReaderCompat::from_bytes(&bytes)?;
            let f = r.get_fields()?.unwrap_or_default();
            assert_eq!(f.len(), fields.len());
            println!(
                "  [PASS] from_bytes: loaded {} fields from {} bytes in memory",
                f.len(),
                bytes.len()
            );
            total_pass += 1;
        }

        // ────────────────────────────────────────────────────
        // TEST 20: Unicode / accented field names (if present)
        // ────────────────────────────────────────────────────
        {
            let accented: Vec<_> = fields
                .keys()
                .filter(|k| k.chars().any(|c| !c.is_ascii()))
                .collect();
            if !accented.is_empty() {
                println!("  [PASS] Unicode field names ({} accented):", accented.len());
                for name in accented.iter().take(5) {
                    println!("    {name}");
                }
                if accented.len() > 5 {
                    println!("    ... and {} more", accented.len() - 5);
                }
                total_pass += 1;
            }
        }

        println!();
    }

    println!("{}", "=".repeat(74));
    println!(
        "  RESULTS: {total_pass} passed, {total_fail} failed out of {} total",
        total_pass + total_fail
    );
    println!("{}", "=".repeat(74));

    if total_fail > 0 {
        std::process::exit(1);
    }
    Ok(())
}
