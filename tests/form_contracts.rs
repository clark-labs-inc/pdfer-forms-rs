use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use pdfer_forms::{field_flags, FieldInput, PageSelection, PdfReaderCompat, PdfWriterCompat};
use std::collections::BTreeMap;

fn name(value: &str) -> Object {
    Object::Name(value.as_bytes().to_vec())
}

fn text(value: &str) -> Object {
    Object::String(value.as_bytes().to_vec(), StringFormat::Literal)
}

fn rect(x: i64) -> Object {
    Object::Array(vec![
        Object::Integer(x),
        Object::Integer(0),
        Object::Integer(x + 20),
        Object::Integer(20),
    ])
}

fn appearance_stream(document: &mut Document, marker: &str) -> ObjectId {
    let mut dictionary = Dictionary::new();
    dictionary.set("Type", name("XObject"));
    dictionary.set("Subtype", name("Form"));
    dictionary.set(
        "BBox",
        Object::Array(vec![0.into(), 0.into(), 20.into(), 20.into()]),
    );
    document.add_object(Object::Stream(Stream::new(
        dictionary,
        marker.as_bytes().to_vec(),
    )))
}

fn button_appearance(
    document: &mut Document,
    on_state: &str,
    marker: &str,
) -> Object {
    let off_id = appearance_stream(document, "off");
    let on_id = appearance_stream(document, marker);
    let mut normal = Dictionary::new();
    normal.set("Off", Object::Reference(off_id));
    normal.set(on_state, Object::Reference(on_id));
    let mut appearance = Dictionary::new();
    appearance.set("N", Object::Dictionary(normal));
    Object::Dictionary(appearance)
}

fn finish_document(
    mut document: Document,
    pages_id: ObjectId,
    page_id: ObjectId,
    fields: Vec<Object>,
) -> Document {
    let mut pages = Dictionary::new();
    pages.set("Type", name("Pages"));
    pages.set("Kids", Object::Array(vec![Object::Reference(page_id)]));
    pages.set("Count", Object::Integer(1));
    document.objects.insert(pages_id, Object::Dictionary(pages));

    let mut acroform = Dictionary::new();
    acroform.set("Fields", Object::Array(fields));
    let acroform_id = document.add_object(Object::Dictionary(acroform));

    let mut catalog = Dictionary::new();
    catalog.set("Type", name("Catalog"));
    catalog.set("Pages", Object::Reference(pages_id));
    catalog.set("AcroForm", Object::Reference(acroform_id));
    let catalog_id = document.add_object(Object::Dictionary(catalog));
    document
        .trailer
        .set("Root", Object::Reference(catalog_id));
    document
}

fn merged_text_document() -> (Document, ObjectId, ObjectId) {
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let parent_id = document.new_object_id();
    let widget_id = document.new_object_id();
    let page_id = document.new_object_id();

    let mut parent = Dictionary::new();
    parent.set("T", text("group"));
    parent.set("Kids", Object::Array(vec![Object::Reference(widget_id)]));
    document
        .objects
        .insert(parent_id, Object::Dictionary(parent));

    let mut widget = Dictionary::new();
    widget.set("Type", name("Annot"));
    widget.set("Subtype", name("Widget"));
    widget.set("FT", name("Tx"));
    widget.set("T", text("name"));
    widget.set("Parent", Object::Reference(parent_id));
    widget.set("Rect", rect(0));
    document
        .objects
        .insert(widget_id, Object::Dictionary(widget));

    let mut page = Dictionary::new();
    page.set("Type", name("Page"));
    page.set("Parent", Object::Reference(pages_id));
    page.set(
        "MediaBox",
        Object::Array(vec![0.into(), 0.into(), 200.into(), 200.into()]),
    );
    page.set("Annots", Object::Array(vec![Object::Reference(widget_id)]));
    document.objects.insert(page_id, Object::Dictionary(page));

    (
        finish_document(
            document,
            pages_id,
            page_id,
            vec![Object::Reference(parent_id)],
        ),
        parent_id,
        widget_id,
    )
}

fn radio_document() -> (Document, ObjectId, ObjectId) {
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let field_id = document.new_object_id();
    let a_id = document.new_object_id();
    let b_id = document.new_object_id();
    let page_id = document.new_object_id();

    let mut field = Dictionary::new();
    field.set("T", text("choice"));
    field.set("FT", name("Btn"));
    field.set("Ff", Object::Integer(field_flags::RADIO as i64));
    field.set(
        "Kids",
        Object::Array(vec![Object::Reference(a_id), Object::Reference(b_id)]),
    );
    document
        .objects
        .insert(field_id, Object::Dictionary(field));

    for (widget_id, state, x) in [(a_id, "A", 0), (b_id, "B", 30)] {
        let appearance = button_appearance(&mut document, state, state);
        let mut widget = Dictionary::new();
        widget.set("Type", name("Annot"));
        widget.set("Subtype", name("Widget"));
        widget.set("Parent", Object::Reference(field_id));
        widget.set("Rect", rect(x));
        widget.set("AP", appearance);
        document
            .objects
            .insert(widget_id, Object::Dictionary(widget));
    }

    let mut page = Dictionary::new();
    page.set("Type", name("Page"));
    page.set("Parent", Object::Reference(pages_id));
    page.set(
        "MediaBox",
        Object::Array(vec![0.into(), 0.into(), 200.into(), 200.into()]),
    );
    // Put B first so an order-dependent implementation incorrectly leaves the
    // parent value at /Off after processing A.
    page.set(
        "Annots",
        Object::Array(vec![Object::Reference(b_id), Object::Reference(a_id)]),
    );
    document.objects.insert(page_id, Object::Dictionary(page));

    (
        finish_document(
            document,
            pages_id,
            page_id,
            vec![Object::Reference(field_id)],
        ),
        field_id,
        page_id,
    )
}

fn separate_checkbox_document(indirect_kids: bool) -> Document {
    let mut document = Document::with_version("1.7");
    let pages_id = document.new_object_id();
    let field_id = document.new_object_id();
    let widget_id = document.new_object_id();
    let page_id = document.new_object_id();

    let kids = Object::Array(vec![Object::Reference(widget_id)]);
    let kids = if indirect_kids {
        Object::Reference(document.add_object(kids))
    } else {
        kids
    };
    let mut field = Dictionary::new();
    field.set("T", text("check"));
    field.set("FT", name("Btn"));
    field.set("Kids", kids);
    document
        .objects
        .insert(field_id, Object::Dictionary(field));

    let appearance = button_appearance(&mut document, "Yes", "yes");
    let mut widget = Dictionary::new();
    widget.set("Type", name("Annot"));
    widget.set("Subtype", name("Widget"));
    widget.set("Parent", Object::Reference(field_id));
    widget.set("Rect", rect(0));
    widget.set("AP", appearance);
    document
        .objects
        .insert(widget_id, Object::Dictionary(widget));

    let mut page = Dictionary::new();
    page.set("Type", name("Page"));
    page.set("Parent", Object::Reference(pages_id));
    page.set(
        "MediaBox",
        Object::Array(vec![0.into(), 0.into(), 200.into(), 200.into()]),
    );
    page.set("Annots", Object::Array(vec![Object::Reference(widget_id)]));
    document.objects.insert(page_id, Object::Dictionary(page));

    finish_document(
        document,
        pages_id,
        page_id,
        vec![Object::Reference(field_id)],
    )
}

#[test]
fn merged_widget_fill_does_not_overwrite_its_nonterminal_parent() {
    let (document, parent_id, widget_id) = merged_text_document();
    let mut writer = PdfWriterCompat::from_document(document);
    let mut updates = BTreeMap::new();
    updates.insert("group.name".to_owned(), FieldInput::from("Ada"));

    writer
        .update_page_form_field_values(PageSelection::All, &updates, 0, Some(false), false)
        .unwrap();

    let parent = writer.document().get_dictionary(parent_id).unwrap();
    assert_eq!(parent.get(b"T").unwrap().as_str().unwrap(), b"group");
    assert_eq!(
        parent.get(b"Kids").unwrap().as_array().unwrap(),
        &[Object::Reference(widget_id)]
    );
    assert!(parent.get(b"Parent").is_err(), "parent must not become self-referential");
}

#[test]
fn radio_fill_keeps_canonical_value_independent_of_widget_order() {
    let (document, field_id, _) = radio_document();
    let mut writer = PdfWriterCompat::from_document(document);
    let mut updates = BTreeMap::new();
    updates.insert("choice".to_owned(), FieldInput::from("B"));

    writer
        .update_page_form_field_values(PageSelection::All, &updates, 0, Some(false), false)
        .unwrap();

    let field = writer.document().get_dictionary(field_id).unwrap();
    assert_eq!(field.get(b"V").unwrap().as_name().unwrap(), b"B");
}

#[test]
fn flatten_uses_a_distinct_xobject_for_each_widget() {
    let (document, _, page_id) = radio_document();
    let mut writer = PdfWriterCompat::from_document(document);
    let mut updates = BTreeMap::new();
    updates.insert("choice".to_owned(), FieldInput::from("B"));

    writer
        .update_page_form_field_values(PageSelection::All, &updates, 0, Some(false), true)
        .unwrap();

    let page = writer.document().get_dictionary(page_id).unwrap();
    let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
    let xobjects = resources.get(b"XObject").unwrap().as_dict().unwrap();
    assert_eq!(xobjects.len(), 2, "each placement must retain its own appearance stream");
}

#[test]
fn discovers_checkbox_states_from_separate_child_widget() {
    let reader = PdfReaderCompat::from_document(separate_checkbox_document(false));
    let fields = reader.get_fields().unwrap().unwrap();

    assert_eq!(fields["check"].states, vec!["/Off", "/Yes"]);
}

#[test]
fn discovers_indirect_kids_and_their_page() {
    let reader = PdfReaderCompat::from_document(separate_checkbox_document(true));
    let fields = reader.get_fields().unwrap().unwrap();

    assert_eq!(fields["check"].kids.len(), 1);
    assert_eq!(fields["check"].states, vec!["/Off", "/Yes"]);
    assert_eq!(reader.get_pages_showing_field("check").unwrap().len(), 1);
}

#[test]
fn reattach_fields_does_not_duplicate_an_existing_descendant_widget() {
    let (document, _, _) = merged_text_document();
    let mut writer = PdfWriterCompat::from_document(document);

    let reattached = writer.reattach_fields(None).unwrap();

    assert!(reattached.is_empty());
    assert_eq!(writer.get_fields().unwrap().unwrap().len(), 2);
}
