use pdfer_forms::{FieldInput, PageSelection, PdfReaderCompat, PdfWriterCompat};
use std::collections::BTreeMap;

fn main() -> pdfer_forms::Result<()> {
    let reader = PdfReaderCompat::load("input.pdf")?;
    println!("text fields: {:#?}", reader.get_form_text_fields(false)?);

    let mut writer = PdfWriterCompat::from_reader(&reader);
    writer.set_need_appearances_writer(false)?;

    let mut updates = BTreeMap::new();
    updates.insert("name".to_owned(), FieldInput::from("Alice Example"));
    updates.insert("city".to_owned(), FieldInput::from(("Paris", "/Helv", 12.0)));
    updates.insert("checkbox_1".to_owned(), FieldInput::from("/Yes"));

    writer.update_page_form_field_values(PageSelection::All, &updates, 0, Some(false), true)?;
    writer.remove_annotations(Some(&["/Widget"]))?;
    writer.save("output.pdf")?;
    Ok(())
}
