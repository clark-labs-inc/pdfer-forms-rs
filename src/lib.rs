
#![forbid(unsafe_code)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::field_reassign_with_default)]
#![doc = include_str!("../README.md")]

use lopdf::{Dictionary, Document, Error as LopdfError, Object, ObjectId, Stream, StringFormat};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

pub mod ops;

pub type Result<T> = std::result::Result<T, PdferError>;

/// Outcome of a form-field update, so callers can tell which requested fields
/// were actually applied instead of assuming success.
///
/// A field counts as `matched` when at least one widget annotation in the
/// selected pages matched the requested name (by fully-qualified name or partial
/// `/T`). Requested names that matched no widget are reported in `unmatched` —
/// the signal an integration should surface as an error rather than reporting a
/// silent no-op as success.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FillReport {
    /// Requested field names that matched at least one widget and were updated.
    pub matched: Vec<String>,
    /// Requested field names that matched no widget (nothing was written).
    pub unmatched: Vec<String>,
    /// Total widget annotations updated (a single field may span several pages).
    pub widgets_updated: usize,
}

impl FillReport {
    /// True when every requested field matched at least one widget.
    pub fn all_matched(&self) -> bool {
        self.unmatched.is_empty()
    }
}

#[derive(Debug)]
pub enum PdferError {
    Lopdf(LopdfError),
    Io(std::io::Error),
    MissingCatalog,
    MissingAcroForm,
    MissingFields,
    MissingPage(usize),
    MissingField(String),
    InvalidField(String),
    InvalidStructure(&'static str),
    Unsupported(&'static str),
    Message(String),
}

impl fmt::Display for PdferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Lopdf(err) => write!(f, "lopdf error: {err}"),
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::MissingCatalog => write!(f, "missing PDF catalog"),
            Self::MissingAcroForm => write!(f, "no /AcroForm dictionary in PDF"),
            Self::MissingFields => write!(f, "no /Fields array in /AcroForm dictionary"),
            Self::MissingPage(index) => write!(f, "page index {index} does not exist"),
            Self::MissingField(name) => write!(f, "field {name:?} was not found"),
            Self::InvalidField(name) => write!(f, "field {name:?} is not valid"),
            Self::InvalidStructure(msg) => write!(f, "invalid PDF object structure: {msg}"),
            Self::Unsupported(msg) => write!(f, "unsupported feature: {msg}"),
            Self::Message(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for PdferError {}

impl From<LopdfError> for PdferError {
    fn from(value: LopdfError) -> Self {
        Self::Lopdf(value)
    }
}

impl From<std::io::Error> for PdferError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PageHandle {
    pub index: usize,
    pub object_id: ObjectId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageSelection {
    All,
    Index(usize),
    Indices(Vec<usize>),
    PageId(ObjectId),
    PageIds(Vec<ObjectId>),
}

impl From<usize> for PageSelection {
    fn from(value: usize) -> Self {
        Self::Index(value)
    }
}

impl From<Vec<usize>> for PageSelection {
    fn from(value: Vec<usize>) -> Self {
        Self::Indices(value)
    }
}

impl From<PageHandle> for PageSelection {
    fn from(value: PageHandle) -> Self {
        Self::PageId(value.object_id)
    }
}

impl From<Vec<PageHandle>> for PageSelection {
    fn from(value: Vec<PageHandle>) -> Self {
        Self::PageIds(value.into_iter().map(|page| page.object_id).collect())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldSpecifier {
    Name(String),
    Id(ObjectId),
}

impl From<String> for FieldSpecifier {
    fn from(value: String) -> Self {
        Self::Name(value)
    }
}

impl From<&str> for FieldSpecifier {
    fn from(value: &str) -> Self {
        Self::Name(value.to_owned())
    }
}

impl From<ObjectId> for FieldSpecifier {
    fn from(value: ObjectId) -> Self {
        Self::Id(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldInput {
    KeepCurrent,
    Text(String),
    TextList(Vec<String>),
    TextAppearance {
        text: String,
        font_name: String,
        font_size: f32,
    },
}

impl FieldInput {
    fn primary_text(&self) -> &str {
        match self {
            Self::KeepCurrent => "",
            Self::Text(text) => text,
            Self::TextList(items) => items.first().map(String::as_str).unwrap_or(""),
            Self::TextAppearance { text, .. } => text,
        }
    }

    fn is_keep_current(&self) -> bool {
        matches!(self, Self::KeepCurrent)
    }

    fn materialize(&self, current_text: Option<&str>) -> Self {
        match self {
            Self::KeepCurrent => Self::Text(current_text.unwrap_or("").to_owned()),
            _ => self.clone(),
        }
    }

    fn text_list(&self) -> Option<&[String]> {
        match self {
            Self::TextList(items) => Some(items.as_slice()),
            _ => None,
        }
    }

    fn font_override(&self) -> Option<(&str, f32)> {
        match self {
            Self::TextAppearance {
                font_name,
                font_size,
                ..
            } => Some((font_name.as_str(), *font_size)),
            _ => None,
        }
    }
}

impl From<String> for FieldInput {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for FieldInput {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

impl From<Vec<String>> for FieldInput {
    fn from(value: Vec<String>) -> Self {
        Self::TextList(value)
    }
}

impl From<(String, String, f32)> for FieldInput {
    fn from(value: (String, String, f32)) -> Self {
        Self::TextAppearance {
            text: value.0,
            font_name: value.1,
            font_size: value.2,
        }
    }
}

impl From<(&str, &str, f32)> for FieldInput {
    fn from(value: (&str, &str, f32)) -> Self {
        Self::TextAppearance {
            text: value.0.to_owned(),
            font_name: value.1.to_owned(),
            font_size: value.2,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    Text(String),
    Name(String),
    List(Vec<String>),
    Null,
}

impl FieldValue {
    fn from_object(obj: &Object) -> Option<Self> {
        match obj {
            Object::String(bytes, _) => Some(Self::Text(decode_pdf_text(bytes))),
            Object::Name(name) => Some(Self::Name(slash_name(&bytes_to_string(name)))),
            Object::Array(items) => {
                let mut values = Vec::new();
                for item in items {
                    match item {
                        Object::String(bytes, _) => values.push(decode_pdf_text(bytes)),
                        Object::Name(name) => values.push(slash_name(&bytes_to_string(name))),
                        other => values.push(object_to_text_lossy(other)),
                    }
                }
                Some(Self::List(values))
            }
            Object::Null => Some(Self::Null),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            Self::Name(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField {
    pub object_id: Option<ObjectId>,
    pub qualified_name: String,
    pub partial_name: Option<String>,
    pub mapping_name: Option<String>,
    pub field_type: Option<String>,
    pub value: Option<FieldValue>,
    pub default_value: Option<FieldValue>,
    pub flags: u32,
    pub states: Vec<String>,
    pub kids: Vec<ObjectId>,
}

impl FormField {
    pub fn is_text_field(&self) -> bool {
        self.field_type.as_deref() == Some("Tx")
    }
}

/// Resolve a requested button value to an exact PDF appearance state.
///
/// Exact states take precedence. Common boolean-like values are accepted for
/// callers that know a checkbox should be selected but do not know whether the
/// document calls its on-state `/Yes`, `/On`, `/1`, or something else.
pub fn resolve_button_state<S: AsRef<str>>(states: &[S], requested: &str) -> Option<String> {
    let normalized_requested = normalize_name(requested);
    let requested = normalized_requested.trim();
    let requested_lower = requested.to_ascii_lowercase();

    if let Some(state) = states
        .iter()
        .map(AsRef::as_ref)
        .find(|state| normalize_name(state).eq_ignore_ascii_case(requested))
    {
        return Some(slash_name(state));
    }

    if matches!(
        requested_lower.as_str(),
        "" | "off" | "false" | "unchecked" | "no" | "0"
    ) {
        return Some("/Off".to_owned());
    }

    if matches!(
        requested_lower.as_str(),
        "on" | "true" | "checked" | "yes" | "1"
    ) {
        return states
            .iter()
            .map(AsRef::as_ref)
            .find(|state| !normalize_name(state).eq_ignore_ascii_case("Off"))
            .map(slash_name);
    }

    None
}

pub mod field_flags {
    pub const READ_ONLY: u32 = 1 << 0;
    pub const REQUIRED: u32 = 1 << 1;
    pub const NO_EXPORT: u32 = 1 << 2;
    pub const NO_TOGGLE_TO_OFF: u32 = 1 << 14;
    pub const RADIO: u32 = 1 << 15;
    pub const PUSHBUTTON: u32 = 1 << 16;
    pub const COMBO: u32 = 1 << 17;
    pub const EDIT: u32 = 1 << 18;
    pub const SORT: u32 = 1 << 19;
    pub const FILE_SELECT: u32 = 1 << 20;
    pub const MULTI_SELECT: u32 = 1 << 21;
    pub const DO_NOT_SPELL_CHECK: u32 = 1 << 22;
    pub const DO_NOT_SCROLL: u32 = 1 << 23;
    pub const COMB: u32 = 1 << 24;
    pub const RICH_TEXT: u32 = 1 << 25;
}

#[derive(Debug, Clone)]
pub struct PdfReaderCompat {
    document: Document,
}

impl PdfReaderCompat {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(Self {
            document: Document::load(path).map_err(PdferError::from)?,
        })
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            document: Document::load_mem(bytes).map_err(PdferError::from)?,
        })
    }

    pub fn from_document(document: Document) -> Self {
        Self { document }
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }

    pub fn into_inner(self) -> Document {
        self.document
    }

    pub fn pages(&self) -> Vec<PageHandle> {
        page_handles(&self.document)
    }

    pub fn page(&self, index: usize) -> Result<PageHandle> {
        page_handle(&self.document, index)
    }

    pub fn get_fields(&self) -> Result<Option<BTreeMap<String, FormField>>> {
        get_fields_impl(&self.document)
    }

    pub fn get_form_text_fields(
        &self,
        full_qualified_name: bool,
    ) -> Result<BTreeMap<String, Option<String>>> {
        let Some(fields) = self.get_fields()? else {
            return Ok(BTreeMap::new());
        };

        let mut text_fields = BTreeMap::new();
        for (qualified_name, field) in fields {
            if field.field_type.as_deref() != Some("Tx") {
                continue;
            }
            let value = match field.value {
                Some(FieldValue::Text(value)) => Some(value),
                Some(FieldValue::Name(value)) => Some(value),
                Some(FieldValue::Null) | None => None,
                Some(FieldValue::List(list)) => Some(list.join(", ")),
            };
            if full_qualified_name {
                text_fields.insert(qualified_name, value);
            } else {
                let base = field.partial_name.unwrap_or_else(|| qualified_name.clone());
                let key = indexed_key(&base, &text_fields);
                text_fields.insert(key, value);
            }
        }
        Ok(text_fields)
    }

    pub fn get_pages_showing_field<F: Into<FieldSpecifier>>(
        &self,
        field: F,
    ) -> Result<Vec<PageHandle>> {
        get_pages_showing_field_impl(&self.document, field.into())
    }

    pub fn add_form_topname(&mut self, name: &str) -> Result<Option<FormField>> {
        if !has_acroform(&self.document)? {
            return Ok(None);
        }
        let catalog_id = catalog_id(&self.document)?;
        let acro_id = ensure_indirect_dictionary_entry(&mut self.document, catalog_id, "AcroForm")?;
        let Some(existing_fields) = read_array_entry_clone(&self.document, acro_id, "Fields")? else {
            return Ok(None);
        };

        let mut interim = Dictionary::new();
        interim.set("T", encode_pdf_text_object(name));
        interim.set("Kids", Object::Array(existing_fields.clone()));
        let interim_id = self.document.add_object(Object::Dictionary(interim.clone()));

        let mut acro = self.document.get_dictionary(acro_id)?.clone();
        acro.set("Fields", Object::Array(vec![Object::Reference(interim_id)]));
        write_dict_object(&mut self.document, acro_id, acro)?;

        for child in existing_fields {
            if let Some(child_id) = object_reference(&child) {
                let mut child_dict = self.document.get_dictionary(child_id)?.clone();
                child_dict.set("Parent", Object::Reference(interim_id));
                write_dict_object(&mut self.document, child_id, child_dict)?;
            }
        }

        let field = build_form_field(&self.document, Some(interim_id), &self.document.get_dictionary(interim_id)?.clone())?;
        Ok(Some(field))
    }

    pub fn rename_form_topname(&mut self, name: &str) -> Result<Option<FormField>> {
        if !has_acroform(&self.document)? {
            return Ok(None);
        }
        let catalog_id = catalog_id(&self.document)?;
        let acro_id = ensure_indirect_dictionary_entry(&mut self.document, catalog_id, "AcroForm")?;
        let Some(fields) = read_array_entry_clone(&self.document, acro_id, "Fields")? else {
            return Ok(None);
        };
        let Some(first) = fields.first() else {
            return Ok(None);
        };
        let Some(first_id) = object_reference(first) else {
            return Ok(None);
        };

        let mut dict = self.document.get_dictionary(first_id)?.clone();
        dict.set("T", encode_pdf_text_object(name));
        write_dict_object(&mut self.document, first_id, dict.clone())?;
        Ok(Some(build_form_field(&self.document, Some(first_id), &dict)?))
    }

    #[allow(non_snake_case)]
    pub fn getFields(&self) -> Result<Option<BTreeMap<String, FormField>>> {
        self.get_fields()
    }

    #[allow(non_snake_case)]
    pub fn getFormTextFields(
        &self,
        full_qualified_name: bool,
    ) -> Result<BTreeMap<String, Option<String>>> {
        self.get_form_text_fields(full_qualified_name)
    }

    #[allow(non_snake_case)]
    pub fn getPagesShowingField<F: Into<FieldSpecifier>>(
        &self,
        field: F,
    ) -> Result<Vec<PageHandle>> {
        self.get_pages_showing_field(field)
    }

    #[allow(non_snake_case)]
    pub fn addFormTopname(&mut self, name: &str) -> Result<Option<FormField>> {
        self.add_form_topname(name)
    }

    #[allow(non_snake_case)]
    pub fn renameFormTopname(&mut self, name: &str) -> Result<Option<FormField>> {
        self.rename_form_topname(name)
    }
}

#[derive(Debug, Clone)]
pub struct PdfWriterCompat {
    document: Document,
}

impl PdfWriterCompat {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(Self {
            document: Document::load(path).map_err(PdferError::from)?,
        })
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        Ok(Self {
            document: Document::load_mem(bytes).map_err(PdferError::from)?,
        })
    }

    pub fn from_reader(reader: &PdfReaderCompat) -> Self {
        Self {
            document: reader.document.clone(),
        }
    }

    pub fn from_document(document: Document) -> Self {
        Self { document }
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn document_mut(&mut self) -> &mut Document {
        &mut self.document
    }

    pub fn into_inner(self) -> Document {
        self.document
    }

    pub fn pages(&self) -> Vec<PageHandle> {
        page_handles(&self.document)
    }

    pub fn page(&self, index: usize) -> Result<PageHandle> {
        page_handle(&self.document, index)
    }

    pub fn get_fields(&self) -> Result<Option<BTreeMap<String, FormField>>> {
        get_fields_impl(&self.document)
    }

    pub fn get_form_text_fields(
        &self,
        full_qualified_name: bool,
    ) -> Result<BTreeMap<String, Option<String>>> {
        PdfReaderCompat::from_document(self.document.clone()).get_form_text_fields(full_qualified_name)
    }

    pub fn get_pages_showing_field<F: Into<FieldSpecifier>>(
        &self,
        field: F,
    ) -> Result<Vec<PageHandle>> {
        get_pages_showing_field_impl(&self.document, field.into())
    }

    pub fn set_need_appearances_writer(&mut self, state: bool) -> Result<()> {
        let catalog_id = catalog_id(&self.document)?;
        let acro_id = ensure_indirect_dictionary_entry(&mut self.document, catalog_id, "AcroForm")?;
        let mut acro = self.document.get_dictionary(acro_id)?.clone();
        acro.set("NeedAppearances", Object::Boolean(state));
        write_dict_object(&mut self.document, acro_id, acro)
    }

    pub fn set_need_appearances_writer_legacy(&mut self) -> Result<()> {
        self.set_need_appearances_writer(true)
    }

    pub fn update_page_form_field_values(
        &mut self,
        page: PageSelection,
        fields: &BTreeMap<String, FieldInput>,
        flags: u32,
        auto_regenerate: Option<bool>,
        flatten: bool,
    ) -> Result<FillReport> {
        if !has_acroform(&self.document)? {
            return Err(PdferError::MissingAcroForm);
        }
        let mut matched: BTreeSet<String> = BTreeSet::new();
        let mut widgets_updated: usize = 0;
        let catalog_id = catalog_id(&self.document)?;
        let acro_id = ensure_indirect_dictionary_entry(&mut self.document, catalog_id, "AcroForm")?;
        if read_array_entry_clone(&self.document, acro_id, "Fields")?.is_none() {
            return Err(PdferError::MissingFields);
        }
        if let Some(state) = auto_regenerate {
            self.set_need_appearances_writer(state)?;
        }

        let page_ids = selected_page_ids(&self.document, page)?;
        for page_id in page_ids {
            let annotations = ensure_indirect_page_annotations(&mut self.document, page_id)?;
            if annotations.is_empty() {
                continue;
            }

            for annotation_object in annotations {
                let annotation_id = object_reference(&annotation_object);
                let mut annotation_dict = deref_dictionary_clone(&self.document, &annotation_object)?;
                if !is_widget(&annotation_dict) {
                    continue;
                }

                let parent_id = annotation_dict
                    .get(b"Parent")
                    .ok()
                    .and_then(object_reference);
                let mut parent_annotation = if annotation_dict.get(b"FT").is_ok() && annotation_dict.get(b"T").is_ok() {
                    annotation_dict.clone()
                } else if let Some(parent_id) = parent_id {
                    self.document.get_dictionary(parent_id)?.clone()
                } else {
                    annotation_dict.clone()
                };

                // The qualified name must identify the field that holds `/V`,
                // which is `parent_annotation`. For a *merged* field+widget (the
                // annotation itself carries `/FT` and `/T`, as in most IRS /
                // USCIS / DMV forms) that field IS this annotation, so its name
                // must include the annotation's own `/T`. Computing it from
                // `parent_id` here dropped the leaf segment, so a fully-qualified
                // name from `get_fields()` (which includes the leaf) never
                // matched — fills silently no-op'd. Use `annotation_id` in the
                // merged case so the names round-trip.
                let is_merged_field_widget =
                    annotation_dict.get(b"FT").is_ok() && annotation_dict.get(b"T").is_ok();
                let qualified_name = if is_merged_field_widget {
                    match annotation_id {
                        Some(id) => qualified_name_from_id(&self.document, id)?,
                        None => qualified_name_from_dict(&self.document, &parent_annotation),
                    }
                } else if let Some(parent_id) = parent_id {
                    qualified_name_from_id(&self.document, parent_id)?
                } else if let Some(annotation_id) = annotation_id {
                    qualified_name_from_id(&self.document, annotation_id)?
                } else {
                    qualified_name_from_dict(&self.document, &parent_annotation)
                };
                let partial_name = parent_annotation
                    .get(b"T")
                    .ok()
                    .and_then(object_to_text);

                let rect = extract_rect(&annotation_dict)?;

                for (field_name, input) in fields {
                    // Match on the fully-qualified name, the partial `/T`, or —
                    // as a fallback — the requested name's final `.`-segment
                    // against this widget's `/T`. Callers (LLM agents) reliably
                    // copy the leaf field name but frequently drop or mangle the
                    // intermediate AcroForm group segments (e.g. request
                    // `topmostSubform[0].Page1[0].f1_07[0]` when the real field is
                    // `topmostSubform[0].Page1[0].Address_ReadOrder[0].f1_07[0]`).
                    // The leaf `/T` identifies the widget; the group path is just
                    // navigation, so a leaf match resolves these without forcing
                    // byte-exact qualified names.
                    let requested_leaf = field_name.rsplit('.').next().unwrap_or(field_name.as_str());
                    let matches = qualified_name == *field_name
                        || partial_name.as_deref() == Some(field_name.as_str())
                        || partial_name.as_deref() == Some(requested_leaf);
                    if !matches {
                        continue;
                    }
                    matched.insert(field_name.clone());
                    widgets_updated += 1;

                    if inherited_name(&self.document, &parent_annotation, "FT").as_deref() == Some("Ch")
                        && parent_annotation.get(b"I").is_ok()
                    {
                        parent_annotation.remove(b"I");
                    }

                    if flags != 0 {
                        annotation_dict.set("Ff", Object::Integer(flags as i64));
                    }

                    let field_type = inherited_name(&self.document, &parent_annotation, "FT")
                        .unwrap_or_default();
                    let current_value_text = inherited_object(&self.document, &parent_annotation, "V")
                        .as_ref()
                        .and_then(FieldValue::from_object)
                        .and_then(|value| match value {
                            FieldValue::Text(text) => Some(text),
                            FieldValue::Name(name) => Some(name),
                            FieldValue::List(list) => Some(list.join(", ")),
                            FieldValue::Null => None,
                        });
                    let effective_input = input.materialize(current_value_text.as_deref());

                    if !input.is_keep_current() {
                        apply_field_value(&mut parent_annotation, &mut annotation_dict, &field_type, &effective_input)?;
                    }

                    let mut appearance_stream_id = None;
                    if field_type == "Btn" {
                        let state_name = choose_button_state(&self.document, &annotation_dict, effective_input.primary_text());
                        let pdf_name = normalize_name(&state_name);
                        annotation_dict.set("AS", Object::Name(pdf_name.as_bytes().to_vec()));
                        annotation_dict.set("V", Object::Name(pdf_name.as_bytes().to_vec()));
                        parent_annotation.set("V", Object::Name(pdf_name.as_bytes().to_vec()));
                        appearance_stream_id = resolve_button_appearance_stream(&mut self.document, &annotation_dict, &state_name)?;
                    } else if field_type == "Tx" || field_type == "Ch" {
                        if let Some(annotation_id) = annotation_id {
                            let stream_id = install_text_appearance(
                                &mut self.document,
                                acro_id,
                                annotation_id,
                                &parent_annotation,
                                &annotation_dict,
                                &effective_input,
                            )?;
                            appearance_stream_id = Some(stream_id);
                            annotation_dict = self.document.get_dictionary(annotation_id)?.clone();
                        }
                    }

                    if let Some(parent_id) = parent_id {
                        write_dict_object(&mut self.document, parent_id, parent_annotation.clone())?;
                    }
                    if let Some(annotation_id) = annotation_id {
                        write_dict_object(&mut self.document, annotation_id, annotation_dict.clone())?;
                    }

                    if flatten {
                        if let Some(ap_id) = appearance_stream_id {
                            let object_name = format!(
                                "Fm_{}_{}_{}",
                                field_name.replace('.', "_"),
                                page_id.0,
                                page_id.1
                            );
                            flatten_appearance_stream(
                                &mut self.document,
                                page_id,
                                ap_id,
                                &object_name,
                                rect[0],
                                rect[1],
                            )?;
                        }
                    }
                }
            }
        }

        let unmatched = fields
            .keys()
            .filter(|name| !matched.contains(*name))
            .cloned()
            .collect();
        Ok(FillReport {
            matched: matched.into_iter().collect(),
            unmatched,
            widgets_updated,
        })
    }

    pub fn reattach_fields(&mut self, page: Option<PageSelection>) -> Result<Vec<FormField>> {
        let selected = match page {
            Some(selection) => selected_page_ids(&self.document, selection)?,
            None => selected_page_ids(&self.document, PageSelection::All)?,
        };

        let catalog_id = catalog_id(&self.document)?;
        let acro_id = ensure_indirect_dictionary_entry(&mut self.document, catalog_id, "AcroForm")?;
        let mut fields = read_array_entry_clone(&self.document, acro_id, "Fields")?.unwrap_or_default();

        let mut existing = BTreeSet::new();
        for item in &fields {
            if let Some(id) = object_reference(item) {
                existing.insert(id);
            }
        }

        let mut reattached = Vec::new();
        for page_id in selected {
            let annotations = page_annotation_objects(&self.document, page_id)?;
            if annotations.is_empty() {
                continue;
            }

            let mut rewritten_annotations = Vec::new();
            let mut annotations_changed = false;

            for annotation in annotations {
                let (annotation_id, annotation_dict) = match annotation.clone() {
                    Object::Reference(id) => (id, self.document.get_dictionary(id)?.clone()),
                    Object::Dictionary(dict) => {
                        let id = self.document.add_object(Object::Dictionary(dict.clone()));
                        annotations_changed = true;
                        (id, dict)
                    }
                    _ => continue,
                };

                rewritten_annotations.push(Object::Reference(annotation_id));

                if !is_widget(&annotation_dict) || annotation_dict.get(b"FT").is_err() {
                    continue;
                }

                if existing.contains(&annotation_id) {
                    continue;
                }

                existing.insert(annotation_id);
                fields.push(Object::Reference(annotation_id));
                reattached.push(build_form_field(&self.document, Some(annotation_id), &annotation_dict)?);
            }

            if annotations_changed {
                write_array_entry(&mut self.document, page_id, "Annots", rewritten_annotations)?;
            }
        }

        write_array_entry(&mut self.document, acro_id, "Fields", fields)?;
        Ok(reattached)
    }

    pub fn remove_annotations(&mut self, subtypes: Option<&[&str]>) -> Result<()> {
        let all_pages = selected_page_ids(&self.document, PageSelection::All)?;
        for page_id in all_pages {
            let annotations = page_annotation_objects(&self.document, page_id)?;
            if annotations.is_empty() {
                continue;
            }

            let mut kept = Vec::new();
            for annotation in annotations {
                let annotation_id = object_reference(&annotation);
                let annotation_dict = match deref_dictionary_clone(&self.document, &annotation) {
                    Ok(dict) => dict,
                    Err(_) => continue, // skip null / invalid annotation refs
                };
                let subtype = annotation_dict
                    .get(b"Subtype")
                    .ok()
                    .and_then(object_name)
                    .map(|n| slash_name(&n))
                    .unwrap_or_default();

                let should_remove = subtypes
                    .map(|items| items.iter().any(|candidate| slash_name(candidate) == subtype))
                    .unwrap_or(true);

                if should_remove {
                    if let Some(annotation_id) = annotation_id {
                        *self.document.get_object_mut(annotation_id)? = Object::Null;
                    }
                } else {
                    kept.push(annotation);
                }
            }

            write_array_entry(&mut self.document, page_id, "Annots", kept)?;
        }
        Ok(())
    }

    pub fn save<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let _ = self.document.save(path).map_err(|e| PdferError::Message(e.to_string()))?;
        Ok(())
    }

    #[allow(non_snake_case)]
    pub fn getFields(&self) -> Result<Option<BTreeMap<String, FormField>>> {
        self.get_fields()
    }

    #[allow(non_snake_case)]
    pub fn getFormTextFields(
        &self,
        full_qualified_name: bool,
    ) -> Result<BTreeMap<String, Option<String>>> {
        self.get_form_text_fields(full_qualified_name)
    }

    #[allow(non_snake_case)]
    pub fn getPagesShowingField<F: Into<FieldSpecifier>>(
        &self,
        field: F,
    ) -> Result<Vec<PageHandle>> {
        self.get_pages_showing_field(field)
    }

    #[allow(non_snake_case)]
    pub fn setNeedAppearancesWriter(&mut self) -> Result<()> {
        self.set_need_appearances_writer_legacy()
    }

    #[allow(non_snake_case)]
    pub fn updatePageFormFieldValues(
        &mut self,
        page: PageSelection,
        fields: &BTreeMap<String, String>,
        flags: u32,
    ) -> Result<()> {
        let converted = fields
            .iter()
            .map(|(name, value)| (name.clone(), FieldInput::Text(value.clone())))
            .collect::<BTreeMap<_, _>>();
        self.update_page_form_field_values(page, &converted, flags, Some(true), false)
            .map(|_| ())
    }

    #[allow(non_snake_case)]
    pub fn reattachFields(&mut self, page: Option<PageSelection>) -> Result<Vec<FormField>> {
        self.reattach_fields(page)
    }
}

fn get_fields_impl(document: &Document) -> Result<Option<BTreeMap<String, FormField>>> {
    let Some(fields) = acroform_fields(document)? else {
        return Ok(None);
    };

    let mut result = BTreeMap::new();
    let mut visited = BTreeSet::new();
    for field in fields {
        collect_fields(document, &field, &mut result, &mut visited)?;
    }
    Ok(Some(result))
}

fn collect_fields(
    document: &Document,
    field_obj: &Object,
    out: &mut BTreeMap<String, FormField>,
    visited: &mut BTreeSet<ObjectId>,
) -> Result<()> {
    let field_id = object_reference(field_obj);
    if let Some(field_id) = field_id {
        if !visited.insert(field_id) {
            return Ok(());
        }
    }

    let field_dict = deref_dictionary_clone(document, field_obj)?;
    if field_dict.get(b"T").is_ok() || field_dict.get(b"TM").is_ok() {
        let built = build_form_field(document, field_id, &field_dict)?;
        out.insert(built.qualified_name.clone(), built);
    }

    if let Ok(kids_obj) = field_dict.get(b"Kids") {
        if let Object::Array(kids) = kids_obj {
            for kid in kids {
                collect_fields(document, kid, out, visited)?;
            }
        }
    }

    Ok(())
}

fn build_form_field(document: &Document, field_id: Option<ObjectId>, dict: &Dictionary) -> Result<FormField> {
    let qualified_name = match field_id {
        Some(id) => qualified_name_from_id(document, id)?,
        None => qualified_name_from_dict(document, dict),
    };
    let partial_name = dict.get(b"T").ok().and_then(object_to_text);
    let mapping_name = dict.get(b"TM").ok().and_then(object_to_text);
    let field_type = inherited_name(document, dict, "FT");
    let value = inherited_object(document, dict, "V").as_ref().and_then(FieldValue::from_object);
    let default_value = inherited_object(document, dict, "DV").as_ref().and_then(FieldValue::from_object);
    let flags = inherited_integer(document, dict, "Ff").unwrap_or_default() as u32;
    let states = field_states(document, dict);
    let kids = dict
        .get(b"Kids")
        .ok()
        .and_then(|obj| match obj {
            Object::Array(values) => Some(
                values
                    .iter()
                    .filter_map(object_reference)
                    .collect::<Vec<ObjectId>>(),
            ),
            _ => None,
        })
        .unwrap_or_default();

    Ok(FormField {
        object_id: field_id,
        qualified_name,
        partial_name,
        mapping_name,
        field_type,
        value,
        default_value,
        flags,
        states,
        kids,
    })
}

fn field_states(document: &Document, dict: &Dictionary) -> Vec<String> {
    let field_type = inherited_name(document, dict, "FT").unwrap_or_default();
    if field_type == "Ch" {
        if let Some(opt) = inherited_object(document, dict, "Opt") {
            return extract_choice_options(&opt);
        }
    }

    if field_type == "Btn" {
        if let Some(states) = extract_button_states_from_dict(document, dict) {
            return states;
        }
    }

    Vec::new()
}

fn extract_choice_options(opt: &Object) -> Vec<String> {
    match opt {
        Object::Array(values) => values
            .iter()
            .filter_map(|value| match value {
                Object::String(bytes, _) => Some(decode_pdf_text(bytes)),
                Object::Array(parts) => parts.first().and_then(|first| match first {
                    Object::String(bytes, _) => Some(decode_pdf_text(bytes)),
                    Object::Name(name) => Some(slash_name(&bytes_to_string(name))),
                    _ => None,
                }),
                Object::Name(name) => Some(slash_name(&bytes_to_string(name))),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn extract_button_states_from_dict(document: &Document, dict: &Dictionary) -> Option<Vec<String>> {
    if let Ok(ap_obj) = dict.get(b"AP") {
        if let Ok(ap_dict) = deref_dictionary_clone(document, ap_obj) {
            if let Ok(normal_obj) = ap_dict.get(b"N") {
                match deref_dictionary_clone(document, normal_obj) {
                    Ok(normal_dict) => {
                        let mut states = normal_dict
                            .iter()
                            .map(|(name, _)| slash_name(&bytes_to_string(name)))
                            .collect::<Vec<_>>();
                        if !states.iter().any(|state| state == "/Off") {
                            states.push("/Off".to_owned());
                        }
                        states.sort();
                        states.dedup();
                        return Some(states);
                    }
                    Err(_) => {}
                }
            }
        }
    }

    let flags = inherited_integer(document, dict, "Ff").unwrap_or_default() as u32;
    if flags & field_flags::RADIO != 0 {
        if let Ok(kids_obj) = dict.get(b"Kids") {
            if let Object::Array(kids) = kids_obj {
                let mut states = Vec::new();
                for kid in kids {
                    if let Ok(kid_dict) = deref_dictionary_clone(document, kid) {
                        if let Some(mut child_states) = extract_button_states_from_dict(document, &kid_dict) {
                            states.append(&mut child_states);
                        }
                    }
                }
                states.sort();
                states.dedup();
                if flags & field_flags::NO_TOGGLE_TO_OFF != 0 {
                    states.retain(|state| state != "/Off");
                }
                return Some(states);
            }
        }
    }

    None
}

fn get_pages_showing_field_impl(document: &Document, field: FieldSpecifier) -> Result<Vec<PageHandle>> {
    let (field_id, field_dict) = match field {
        FieldSpecifier::Id(id) => (Some(id), document.get_dictionary(id)?.clone()),
        FieldSpecifier::Name(name) => {
            let Some(fields) = get_fields_impl(document)? else {
                return Err(PdferError::MissingField(name));
            };
            let Some(form_field) = fields
                .get(&name)
                .or_else(|| fields.values().find(|field| field.partial_name.as_deref() == Some(name.as_str())))
            else {
                return Err(PdferError::MissingField(name));
            };
            let Some(id) = form_field.object_id else {
                return Err(PdferError::InvalidField(form_field.qualified_name.clone()));
            };
            (Some(id), document.get_dictionary(id)?.clone())
        }
    };

    if inherited_name(document, &field_dict, "FT").is_none() {
        return Err(PdferError::InvalidField(
            field_id
                .map(|id| format!("{} {}", id.0, id.1))
                .unwrap_or_else(|| "field".to_owned()),
        ));
    }

    let mut page_ids = BTreeSet::new();

    if object_name_from_dict(&field_dict, "Subtype").as_deref() == Some("Widget") {
        if let Ok(page_obj) = field_dict.get(b"P") {
            if let Some(page_id) = object_reference(page_obj) {
                page_ids.insert(page_id);
            }
        } else if let Some(field_id) = field_id {
            for page in page_handles(document) {
                let annots = page_annotation_objects(document, page.object_id)?;
                if annots.iter().any(|annot| object_reference(annot) == Some(field_id)) {
                    page_ids.insert(page.object_id);
                }
            }
        }
    } else if let Ok(kids_obj) = field_dict.get(b"Kids") {
        if let Object::Array(kids) = kids_obj {
            for kid in kids {
                let kid_dict = deref_dictionary_clone(document, kid)?;
                if object_name_from_dict(&kid_dict, "Subtype").as_deref() == Some("Widget")
                    && kid_dict.get(b"T").is_err()
                {
                    if let Ok(page_obj) = kid_dict.get(b"P") {
                        if let Some(page_id) = object_reference(page_obj) {
                            page_ids.insert(page_id);
                            continue;
                        }
                    }
                    if let Some(kid_id) = object_reference(kid) {
                        for page in page_handles(document) {
                            let annots = page_annotation_objects(document, page.object_id)?;
                            if annots.iter().any(|annot| object_reference(annot) == Some(kid_id)) {
                                page_ids.insert(page.object_id);
                            }
                        }
                    }
                }
            }
        }
    }

    let handles = page_handles(document)
        .into_iter()
        .filter(|page| page_ids.contains(&page.object_id))
        .collect();
    Ok(handles)
}

fn apply_field_value(
    parent_annotation: &mut Dictionary,
    annotation_dict: &mut Dictionary,
    field_type: &str,
    input: &FieldInput,
) -> Result<()> {
    match field_type {
        "Btn" => {
            let state_name = normalize_name(input.primary_text());
            let name_object = Object::Name(state_name.as_bytes().to_vec());
            parent_annotation.set("V", name_object.clone());
            annotation_dict.set("V", name_object);
        }
        "Tx" | "Ch" => {
            if let Some(list) = input.text_list() {
                let values: Vec<Object> = list.iter().map(|item| encode_pdf_text_object(item)).collect();
                annotation_dict.set("V", Object::Array(values.clone()));
                parent_annotation.set("V", Object::Array(values));
            } else {
                let encoded = encode_pdf_text_object(input.primary_text());
                annotation_dict.set("V", encoded.clone());
                parent_annotation.set("V", encoded);
            }
        }
        "Sig" => {}
        _ => {}
    }
    Ok(())
}

fn install_text_appearance(
    document: &mut Document,
    acro_id: ObjectId,
    annotation_id: ObjectId,
    parent_annotation: &Dictionary,
    annotation_dict: &Dictionary,
    input: &FieldInput,
) -> Result<ObjectId> {
    let stream = build_text_appearance_stream(document, acro_id, parent_annotation, annotation_dict, input)?;
    let stream_id = document.add_object(Object::Stream(stream));

    let mut annotation_out = annotation_dict.clone();
    let mut appearance_dict = match annotation_out.get(b"AP") {
        Ok(ap_obj) => deref_dictionary_clone(document, ap_obj).unwrap_or_else(|_| Dictionary::new()),
        Err(_) => Dictionary::new(),
    };
    appearance_dict.set("N", Object::Reference(stream_id));
    annotation_out.set("AP", Object::Dictionary(appearance_dict));
    write_dict_object(document, annotation_id, annotation_out)?;

    Ok(stream_id)
}

fn build_text_appearance_stream(
    document: &mut Document,
    acro_id: ObjectId,
    parent_annotation: &Dictionary,
    annotation_dict: &Dictionary,
    input: &FieldInput,
) -> Result<Stream> {
    let rect = extract_rect(annotation_dict)?;
    let width = (rect[2] - rect[0]).max(1.0);
    let height = (rect[3] - rect[1]).max(1.0);

    let da = annotation_dict
        .get(b"DA")
        .ok()
        .and_then(object_to_text)
        .or_else(|| parent_annotation.get(b"DA").ok().and_then(object_to_text))
        .or_else(|| {
            document
                .get_dictionary(acro_id)
                .ok()
                .and_then(|acro| acro.get(b"DA").ok().and_then(object_to_text))
        })
        .unwrap_or_else(|| "/Helv 0 Tf 0 g".to_owned());

    let (default_font_name, default_font_size) = parse_default_appearance(&da);
    let (font_name, font_size) = input
        .font_override()
        .map(|(name, size)| (name.to_owned(), size))
        .unwrap_or((default_font_name, default_font_size));

    let font_name = if font_name.is_empty() {
        "Helv".to_owned()
    } else {
        normalize_name(&font_name)
    };
    let effective_font_size = if font_size <= 0.0 {
        (height - 4.0).max(4.0)
    } else {
        font_size
    };
    let font_ref = ensure_core_font_resource(document, acro_id, &font_name)?;

    let mut resources = Dictionary::new();
    let mut font_dict = Dictionary::new();
    font_dict.set(font_name.clone(), Object::Reference(font_ref));
    resources.set("Font", Object::Dictionary(font_dict));

    let baseline = ((height - effective_font_size) / 2.0).max(1.5);
    let text = input.primary_text();
    let escaped_text = escape_content_text(text);
    let stream_source = format!(
        "q\nBT\n/{font_name} {effective_font_size} Tf\n2 {baseline} Td\n({escaped_text}) Tj\nET\nQ\n"
    );

    let mut dict = Dictionary::new();
    dict.set("Type", "XObject");
    dict.set("Subtype", "Form");
    dict.set("FormType", 1_i64);
    dict.set(
        "BBox",
        vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Real(width),
            Object::Real(height),
        ],
    );
    dict.set("Resources", Object::Dictionary(resources));

    Ok(Stream::new(dict, stream_source.into_bytes()))
}

fn ensure_core_font_resource(document: &mut Document, acro_id: ObjectId, font_name: &str) -> Result<ObjectId> {
    let dr_id = ensure_indirect_dictionary_entry(document, acro_id, "DR")?;
    let mut dr = document.get_dictionary(dr_id)?.clone();

    let existing_font_entry = dr.get(b"Font").ok().cloned();
    let normalized_key = normalize_name(font_name);

    match existing_font_entry {
        Some(Object::Reference(font_dict_id)) => {
            let mut font_dict = document.get_dictionary(font_dict_id)?.clone();
            if let Ok(existing) = font_dict.get(normalized_key.as_bytes()) {
                if let Some(existing_ref) = object_reference(existing) {
                    return Ok(existing_ref);
                }
                if let Object::Dictionary(existing_dict) = existing {
                    let new_id = document.add_object(Object::Dictionary(existing_dict.clone()));
                    font_dict.set(normalized_key.clone(), Object::Reference(new_id));
                    write_dict_object(document, font_dict_id, font_dict)?;
                    return Ok(new_id);
                }
            }
            let font_object_id = add_core_font_object(document, &normalized_key);
            font_dict.set(normalized_key.clone(), Object::Reference(font_object_id));
            write_dict_object(document, font_dict_id, font_dict)?;
            Ok(font_object_id)
        }
        Some(Object::Dictionary(mut font_dict)) => {
            if let Ok(existing) = font_dict.get(normalized_key.as_bytes()) {
                if let Some(existing_ref) = object_reference(existing) {
                    return Ok(existing_ref);
                }
            }
            let font_object_id = add_core_font_object(document, &normalized_key);
            font_dict.set(normalized_key.clone(), Object::Reference(font_object_id));
            dr.set("Font", Object::Dictionary(font_dict));
            write_dict_object(document, dr_id, dr)?;
            Ok(font_object_id)
        }
        _ => {
            let font_object_id = add_core_font_object(document, &normalized_key);
            let mut font_dict = Dictionary::new();
            font_dict.set(normalized_key.clone(), Object::Reference(font_object_id));
            dr.set("Font", Object::Dictionary(font_dict));
            write_dict_object(document, dr_id, dr)?;
            Ok(font_object_id)
        }
    }
}

fn add_core_font_object(document: &mut Document, font_name: &str) -> ObjectId {
    let normalized = normalize_name(font_name);
    let base_font = match normalized.as_str() {
        "Helv" | "Helvetica" => "Helvetica".to_owned(),
        "TiRo" | "Times-Roman" => "Times-Roman".to_owned(),
        "Cour" | "Courier" => "Courier".to_owned(),
        "Symb" | "Symbol" => "Symbol".to_owned(),
        "ZaDb" | "ZapfDingbats" => "ZapfDingbats".to_owned(),
        other => other.to_owned(),
    };

    let mut font = Dictionary::new();
    font.set("Type", "Font");
    font.set("Subtype", "Type1");
    font.set("BaseFont", base_font);
    font.set("Encoding", "WinAnsiEncoding");
    document.add_object(Object::Dictionary(font))
}

fn choose_button_state(document: &Document, annotation_dict: &Dictionary, requested: &str) -> String {
    let states = extract_button_states_from_dict(document, annotation_dict).unwrap_or_default();
    resolve_button_state(&states, requested).unwrap_or_else(|| "/Off".to_owned())
}

fn resolve_button_appearance_stream(
    document: &mut Document,
    annotation_dict: &Dictionary,
    state_name: &str,
) -> Result<Option<ObjectId>> {
    let Ok(ap_obj) = annotation_dict.get(b"AP") else {
        return Ok(None);
    };
    let ap_dict = deref_dictionary_clone(document, ap_obj)?;
    let Ok(normal_obj) = ap_dict.get(b"N") else {
        return Ok(None);
    };

    match normal_obj {
        Object::Reference(id) => {
            let stream_object = document.get_object(*id)?.clone();
            match stream_object {
                Object::Stream(_) => Ok(Some(*id)),
                Object::Dictionary(normal_dict) => {
                    let key = normalize_name(state_name);
                    if let Ok(entry) = normal_dict.get(key.as_bytes()) {
                        ensure_stream_reference(document, entry)
                    } else if let Ok(entry) = normal_dict.get(b"Off") {
                        ensure_stream_reference(document, entry)
                    } else {
                        Ok(None)
                    }
                }
                _ => Ok(None),
            }
        }
        Object::Dictionary(normal_dict) => {
            let key = normalize_name(state_name);
            if let Ok(entry) = normal_dict.get(key.as_bytes()) {
                ensure_stream_reference(document, entry)
            } else if let Ok(entry) = normal_dict.get(b"Off") {
                ensure_stream_reference(document, entry)
            } else {
                Ok(None)
            }
        }
        Object::Stream(_) => {
            let id = document.add_object(normal_obj.clone());
            Ok(Some(id))
        }
        _ => Ok(None),
    }
}

fn ensure_stream_reference(document: &mut Document, object: &Object) -> Result<Option<ObjectId>> {
    match object {
        Object::Reference(id) => Ok(Some(*id)),
        Object::Stream(_) => Ok(Some(document.add_object(object.clone()))),
        _ => Ok(None),
    }
}

fn flatten_appearance_stream(
    document: &mut Document,
    page_id: ObjectId,
    appearance_stream_id: ObjectId,
    object_name: &str,
    x_offset: f32,
    y_offset: f32,
) -> Result<()> {
    let mut page = document.get_dictionary(page_id)?.clone();
    let mut resources = match page.get(b"Resources") {
        Ok(resources_obj) => deref_dictionary_clone(document, resources_obj).unwrap_or_else(|_| Dictionary::new()),
        Err(_) => Dictionary::new(),
    };

    let mut xobjects = match resources.get(b"XObject") {
        Ok(x_obj) => deref_dictionary_clone(document, x_obj).unwrap_or_else(|_| Dictionary::new()),
        Err(_) => Dictionary::new(),
    };
    xobjects.set(object_name, Object::Reference(appearance_stream_id));
    resources.set("XObject", Object::Dictionary(xobjects));
    page.set("Resources", Object::Dictionary(resources));
    write_dict_object(document, page_id, page)?;

    let commands = format!(
        "q\n1 0 0 1 {x_offset} {y_offset} cm\n/{object_name} Do\nQ\n"
    );
    document.add_page_contents(page_id, commands.into_bytes())?;
    Ok(())
}

fn indexed_key(base: &str, fields: &BTreeMap<String, Option<String>>) -> String {
    if !fields.contains_key(base) {
        return base.to_owned();
    }
    let count = fields
        .keys()
        .filter(|existing| existing.starts_with(&format!("{base}.")))
        .count();
    format!("{base}.{}", count + 2)
}

fn page_handles(document: &Document) -> Vec<PageHandle> {
    let mut handles = Vec::new();
    for (page_number, object_id) in document.get_pages() {
        handles.push(PageHandle {
            index: page_number.saturating_sub(1) as usize,
            object_id,
        });
    }
    handles.sort_by_key(|page| page.index);
    handles
}

fn page_handle(document: &Document, index: usize) -> Result<PageHandle> {
    page_handles(document)
        .into_iter()
        .find(|page| page.index == index)
        .ok_or(PdferError::MissingPage(index))
}

fn selected_page_ids(document: &Document, selection: PageSelection) -> Result<Vec<ObjectId>> {
    let handles = page_handles(document);
    match selection {
        PageSelection::All => Ok(handles.into_iter().map(|page| page.object_id).collect()),
        PageSelection::Index(index) => Ok(vec![page_handle(document, index)?.object_id]),
        PageSelection::Indices(indices) => indices
            .into_iter()
            .map(|index| page_handle(document, index).map(|page| page.object_id))
            .collect(),
        PageSelection::PageId(id) => Ok(vec![id]),
        PageSelection::PageIds(ids) => Ok(ids),
    }
}


fn ensure_indirect_page_annotations(document: &mut Document, page_id: ObjectId) -> Result<Vec<Object>> {
    let annotations = page_annotation_objects(document, page_id)?;
    let mut rewritten = Vec::with_capacity(annotations.len());
    let mut changed = false;

    for annotation in annotations {
        match annotation {
            Object::Reference(_) => rewritten.push(annotation),
            Object::Dictionary(dict) => {
                let id = document.add_object(Object::Dictionary(dict));
                rewritten.push(Object::Reference(id));
                changed = true;
            }
            other => rewritten.push(other),
        }
    }

    if changed {
        write_array_entry(document, page_id, "Annots", rewritten.clone())?;
    }

    Ok(rewritten)
}

fn page_annotation_objects(document: &Document, page_id: ObjectId) -> Result<Vec<Object>> {
    let page = document.get_dictionary(page_id)?;
    let annots_obj = match page.get(b"Annots") {
        Ok(obj) => obj,
        Err(_) => return Ok(Vec::new()),
    };

    match annots_obj {
        Object::Array(values) => Ok(values.clone()),
        Object::Reference(id) => match document.get_object(*id)? {
            Object::Array(values) => Ok(values.clone()),
            _ => Err(PdferError::InvalidStructure("/Annots must resolve to an array")),
        },
        _ => Err(PdferError::InvalidStructure("/Annots must be an array or indirect array")),
    }
}

fn catalog_id(document: &Document) -> Result<ObjectId> {
    document
        .trailer
        .get(b"Root")
        .ok()
        .and_then(object_reference)
        .ok_or(PdferError::MissingCatalog)
}

fn get_acroform_id(document: &Document) -> Result<Option<ObjectId>> {
    let catalog_id = catalog_id(document)?;
    let catalog = document.get_dictionary(catalog_id)?;
    match catalog.get(b"AcroForm") {
        Ok(Object::Reference(id)) => Ok(Some(*id)),
        Ok(Object::Dictionary(_)) => Ok(None),
        Ok(_) => Err(PdferError::InvalidStructure("/AcroForm is not a dictionary")),
        Err(_) => Ok(None),
    }
}

fn has_acroform(document: &Document) -> Result<bool> {
    let catalog_id = catalog_id(document)?;
    let catalog = document.get_dictionary(catalog_id)?;
    Ok(catalog.get(b"AcroForm").is_ok())
}

fn acroform_fields(document: &Document) -> Result<Option<Vec<Object>>> {
    let catalog_id = catalog_id(document)?;
    let catalog = document.get_dictionary(catalog_id)?;
    let Ok(acro_obj) = catalog.get(b"AcroForm") else {
        return Ok(None);
    };
    let acro_dict = deref_dictionary_clone(document, acro_obj)?;
    let Ok(fields_obj) = acro_dict.get(b"Fields") else {
        return Ok(None);
    };
    match fields_obj {
        Object::Array(values) => Ok(Some(values.clone())),
        Object::Reference(id) => match document.get_object(*id)? {
            Object::Array(values) => Ok(Some(values.clone())),
            _ => Err(PdferError::InvalidStructure("/Fields did not resolve to an array")),
        },
        _ => Err(PdferError::InvalidStructure("/Fields is not an array")),
    }
}

fn ensure_indirect_dictionary_entry(
    document: &mut Document,
    owner_id: ObjectId,
    key: &str,
) -> Result<ObjectId> {
    let existing = document
        .get_dictionary(owner_id)?
        .get(key.as_bytes())
        .ok()
        .cloned();

    match existing {
        Some(Object::Reference(id)) => Ok(id),
        Some(Object::Dictionary(dict)) => {
            let new_id = document.add_object(Object::Dictionary(dict));
            let mut owner = document.get_dictionary(owner_id)?.clone();
            owner.set(key, Object::Reference(new_id));
            write_dict_object(document, owner_id, owner)?;
            Ok(new_id)
        }
        Some(_) => Err(PdferError::InvalidStructure("dictionary entry is not a dictionary")),
        None => {
            let new_id = document.add_object(Object::Dictionary(Dictionary::new()));
            let mut owner = document.get_dictionary(owner_id)?.clone();
            owner.set(key, Object::Reference(new_id));
            write_dict_object(document, owner_id, owner)?;
            Ok(new_id)
        }
    }
}

fn read_array_entry_clone(document: &Document, owner_id: ObjectId, key: &str) -> Result<Option<Vec<Object>>> {
    let owner = document.get_dictionary(owner_id)?;
    let Ok(object) = owner.get(key.as_bytes()) else {
        return Ok(None);
    };
    match object {
        Object::Array(values) => Ok(Some(values.clone())),
        Object::Reference(id) => match document.get_object(*id)? {
            Object::Array(values) => Ok(Some(values.clone())),
            _ => Err(PdferError::InvalidStructure("entry did not resolve to array")),
        },
        _ => Err(PdferError::InvalidStructure("entry was not array or indirect array")),
    }
}

fn write_array_entry(document: &mut Document, owner_id: ObjectId, key: &str, values: Vec<Object>) -> Result<()> {
    let current = document
        .get_dictionary(owner_id)?
        .get(key.as_bytes())
        .ok()
        .cloned();

    match current {
        Some(Object::Reference(id)) => {
            *document.get_object_mut(id)? = Object::Array(values);
            Ok(())
        }
        _ => {
            let mut owner = document.get_dictionary(owner_id)?.clone();
            owner.set(key, Object::Array(values));
            write_dict_object(document, owner_id, owner)
        }
    }
}

fn deref_dictionary_clone(document: &Document, object: &Object) -> Result<Dictionary> {
    match object {
        Object::Dictionary(dict) => Ok(dict.clone()),
        Object::Reference(id) => Ok(document.get_dictionary(*id)?.clone()),
        _ => Err(PdferError::InvalidStructure("object is not a dictionary")),
    }
}

fn write_dict_object(document: &mut Document, object_id: ObjectId, dict: Dictionary) -> Result<()> {
    *document.get_object_mut(object_id)? = Object::Dictionary(dict);
    Ok(())
}

fn object_reference(object: &Object) -> Option<ObjectId> {
    match object {
        Object::Reference(id) => Some(*id),
        _ => None,
    }
}

fn object_name(object: &Object) -> Option<String> {
    match object {
        Object::Name(name) => Some(bytes_to_string(name)),
        _ => None,
    }
}

fn object_name_from_dict(dict: &Dictionary, key: &str) -> Option<String> {
    dict.get(key.as_bytes()).ok().and_then(object_name)
}

fn object_to_text(object: &Object) -> Option<String> {
    match object {
        Object::String(bytes, _) => Some(decode_pdf_text(bytes)),
        Object::Name(name) => Some(bytes_to_string(name)),
        _ => None,
    }
}

fn object_to_text_lossy(object: &Object) -> String {
    object_to_text(object).unwrap_or_else(|| format!("{object:?}"))
}

fn inherited_object(document: &Document, dict: &Dictionary, key: &str) -> Option<Object> {
    if let Ok(obj) = dict.get(key.as_bytes()) {
        return Some(obj.clone());
    }
    let parent_id = dict.get(b"Parent").ok().and_then(object_reference)?;
    let parent = document.get_dictionary(parent_id).ok()?.clone();
    inherited_object(document, &parent, key)
}

fn inherited_name(document: &Document, dict: &Dictionary, key: &str) -> Option<String> {
    inherited_object(document, dict, key).as_ref().and_then(object_name)
}

fn inherited_integer(document: &Document, dict: &Dictionary, key: &str) -> Option<i64> {
    match inherited_object(document, dict, key)? {
        Object::Integer(value) => Some(value),
        _ => None,
    }
}

fn qualified_name_from_id(document: &Document, object_id: ObjectId) -> Result<String> {
    let mut visited = BTreeSet::new();
    qualified_name_from_id_inner(document, object_id, &mut visited)
}

fn qualified_name_from_id_inner(
    document: &Document,
    object_id: ObjectId,
    visited: &mut BTreeSet<ObjectId>,
) -> Result<String> {
    if !visited.insert(object_id) {
        return Ok(String::new());
    }

    let dict = document.get_dictionary(object_id)?.clone();
    if let Some(mapping) = dict.get(b"TM").ok().and_then(object_to_text) {
        return Ok(mapping);
    }

    let local = dict.get(b"T").ok().and_then(object_to_text).unwrap_or_default();
    let parent_id = dict.get(b"Parent").ok().and_then(object_reference);
    if let Some(parent_id) = parent_id {
        let parent_name = qualified_name_from_id_inner(document, parent_id, visited)?;
        if parent_name.is_empty() {
            Ok(local)
        } else if local.is_empty() {
            Ok(parent_name)
        } else {
            Ok(format!("{parent_name}.{local}"))
        }
    } else {
        Ok(local)
    }
}

fn qualified_name_from_dict(document: &Document, dict: &Dictionary) -> String {
    if let Some(mapping) = dict.get(b"TM").ok().and_then(object_to_text) {
        return mapping;
    }
    let local = dict.get(b"T").ok().and_then(object_to_text).unwrap_or_default();
    if let Some(parent_id) = dict.get(b"Parent").ok().and_then(object_reference) {
        let parent_name = qualified_name_from_id(document, parent_id).unwrap_or_default();
        if parent_name.is_empty() {
            local
        } else if local.is_empty() {
            parent_name
        } else {
            format!("{parent_name}.{local}")
        }
    } else {
        local
    }
}

fn parse_default_appearance(da: &str) -> (String, f32) {
    let mut tokens = da.split_whitespace().peekable();
    let mut font_name = "Helv".to_owned();
    let mut font_size = 0.0_f32;

    while let Some(token) = tokens.next() {
        if token == "Tf" {
            continue;
        }

        if token.starts_with('/') {
            let name = normalize_name(token);
            if let Some(next) = tokens.peek() {
                if let Ok(size) = next.parse::<f32>() {
                    font_name = name;
                    font_size = size;
                    let _ = tokens.next();
                    if let Some(op) = tokens.peek() {
                        if *op == "Tf" {
                            let _ = tokens.next();
                        }
                    }
                }
            }
        }
    }

    (font_name, font_size)
}

fn extract_rect(dict: &Dictionary) -> Result<[f32; 4]> {
    let rect_obj = dict
        .get(b"Rect")
        .map_err(|_| PdferError::InvalidStructure("widget is missing /Rect"))?;
    let Object::Array(values) = rect_obj else {
        return Err(PdferError::InvalidStructure("/Rect is not an array"));
    };
    if values.len() != 4 {
        return Err(PdferError::InvalidStructure("/Rect must have four numbers"));
    }

    let mut rect = [0.0_f32; 4];
    for (slot, value) in rect.iter_mut().zip(values.iter()) {
        *slot = match value {
            Object::Integer(number) => *number as f32,
            Object::Real(number) => *number,
            _ => return Err(PdferError::InvalidStructure("/Rect contains non-number value")),
        };
    }
    Ok(rect)
}

fn is_widget(dict: &Dictionary) -> bool {
    object_name_from_dict(dict, "Subtype").as_deref() == Some("Widget")
}

fn normalize_name(name: &str) -> String {
    name.trim_start_matches('/').to_owned()
}

fn slash_name(name: &str) -> String {
    let normalized = normalize_name(name);
    format!("/{normalized}")
}

fn bytes_to_string(bytes: &[u8]) -> String {
    decode_pdf_doc_encoding(bytes)
}

fn decode_pdf_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF && bytes.len() % 2 == 0 {
        let mut units = Vec::with_capacity((bytes.len() - 2) / 2);
        let mut index = 2;
        while index + 1 < bytes.len() {
            units.push(u16::from_be_bytes([bytes[index], bytes[index + 1]]));
            index += 2;
        }
        return String::from_utf16_lossy(&units);
    }
    decode_pdf_doc_encoding(bytes)
}

/// Decode bytes using PDFDocEncoding (ISO 8859-1 / Latin-1 superset).
/// Try UTF-8 first; if invalid, fall back to Latin-1 (1:1 byte-to-codepoint mapping).
fn decode_pdf_doc_encoding(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

fn encode_pdf_text_object(text: &str) -> Object {
    let mut bytes = Vec::with_capacity(text.len() * 2 + 2);
    bytes.extend_from_slice(&[0xFE, 0xFF]);
    for code_unit in text.encode_utf16() {
        bytes.extend_from_slice(&code_unit.to_be_bytes());
    }
    Object::String(bytes, StringFormat::Hexadecimal)
}

fn escape_content_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => escaped.push_str(r"\\"),
            '(' => escaped.push_str(r"\("),
            ')' => escaped.push_str(r"\)"),
            '\r' | '\n' => escaped.push(' '),
            ch if ch.is_ascii() => escaped.push(ch),
            _ => escaped.push('?'),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_and_slash_name_roundtrip() {
        assert_eq!(normalize_name("/Yes"), "Yes");
        assert_eq!(slash_name("Yes"), "/Yes");
        assert_eq!(slash_name("/Yes"), "/Yes");
    }

    #[test]
    fn button_state_resolution_prefers_exact_states_and_supports_boolean_aliases() {
        let states = vec!["/Off".to_owned(), "/Selected".to_owned()];

        assert_eq!(
            resolve_button_state(&states, "/selected"),
            Some("/Selected".to_owned())
        );
        assert_eq!(
            resolve_button_state(&states, "on"),
            Some("/Selected".to_owned())
        );
        assert_eq!(
            resolve_button_state(&states, "true"),
            Some("/Selected".to_owned())
        );
        assert_eq!(
            resolve_button_state(&states, "unchecked"),
            Some("/Off".to_owned())
        );
        assert_eq!(resolve_button_state(&states, "unknown"), None);

        let radio_states = vec!["/No".to_owned(), "/Off".to_owned(), "/Yes".to_owned()];
        assert_eq!(
            resolve_button_state(&radio_states, "no"),
            Some("/No".to_owned())
        );
    }

    #[test]
    fn choose_button_state_maps_on_to_the_widgets_non_off_appearance() {
        let mut normal = Dictionary::new();
        normal.set("Yes", Object::Null);
        let mut appearance = Dictionary::new();
        appearance.set("N", Object::Dictionary(normal));
        let mut widget = Dictionary::new();
        widget.set("FT", Object::Name(b"Btn".to_vec()));
        widget.set("AP", Object::Dictionary(appearance));

        assert_eq!(
            choose_button_state(&Document::with_version("1.7"), &widget, "on"),
            "/Yes"
        );
    }

    #[test]
    fn parse_default_appearance_reads_font_and_size() {
        let (font, size) = parse_default_appearance("/Helv 9 Tf 0 g");
        assert_eq!(font, "Helv");
        assert_eq!(size, 9.0);
    }

    #[test]
    fn decode_pdf_text_handles_utf16be_bom() {
        let obj = encode_pdf_text_object("Hello");
        match obj {
            Object::String(bytes, _) => assert_eq!(decode_pdf_text(&bytes), "Hello"),
            other => panic!("unexpected object: {other:?}"),
        }
    }

    #[test]
    fn indexed_key_matches_pypdf_suffixing() {
        let mut fields = BTreeMap::new();
        fields.insert("city".to_owned(), Some("Berlin".to_owned()));
        assert_eq!(indexed_key("city", &fields), "city.2");
        fields.insert("city.2".to_owned(), Some("Paris".to_owned()));
        assert_eq!(indexed_key("city", &fields), "city.3");
    }

    #[test]
    fn keep_current_materializes_existing_value() {
        let input = FieldInput::KeepCurrent;
        let resolved = input.materialize(Some("existing"));
        assert_eq!(resolved, FieldInput::Text("existing".to_owned()));
    }

    /// Builds a nested AcroForm whose leaf is a *merged* field+widget (carries
    /// `/FT` and `/T` and is the page annotation), exactly the shape used by IRS
    /// / USCIS / DMV forms. Regression for the bug where filling by the
    /// fully-qualified name (`topmostSubform[0].Page1[0].f1_07[0]`) — the name
    /// `get_fields()` returns — silently matched nothing because the qualified
    /// name was computed from the widget's `/Parent` (dropping the leaf `/T`).
    #[test]
    fn fills_merged_widget_by_fully_qualified_name() {
        fn set(dict: &mut Dictionary, k: &str, v: Object) {
            dict.set(k, v);
        }
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let top_id = doc.new_object_id();
        let p_id = doc.new_object_id();
        let widget_id = doc.new_object_id();
        let page_id = doc.new_object_id();

        let mut top = Dictionary::new();
        set(&mut top, "T", Object::string_literal("topmostSubform[0]"));
        set(&mut top, "Kids", Object::Array(vec![Object::Reference(p_id)]));
        doc.objects.insert(top_id, Object::Dictionary(top));

        let mut parent = Dictionary::new();
        set(&mut parent, "T", Object::string_literal("Page1[0]"));
        set(&mut parent, "Parent", Object::Reference(top_id));
        set(&mut parent, "Kids", Object::Array(vec![Object::Reference(widget_id)]));
        doc.objects.insert(p_id, Object::Dictionary(parent));

        let mut widget = Dictionary::new();
        set(&mut widget, "Type", Object::Name(b"Annot".to_vec()));
        set(&mut widget, "Subtype", Object::Name(b"Widget".to_vec()));
        set(&mut widget, "FT", Object::Name(b"Tx".to_vec()));
        set(&mut widget, "T", Object::string_literal("f1_07[0]"));
        set(&mut widget, "Parent", Object::Reference(p_id));
        set(
            &mut widget,
            "Rect",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(150),
                Object::Integer(20),
            ]),
        );
        doc.objects.insert(widget_id, Object::Dictionary(widget));

        let mut page = Dictionary::new();
        set(&mut page, "Type", Object::Name(b"Page".to_vec()));
        set(&mut page, "Parent", Object::Reference(pages_id));
        set(
            &mut page,
            "MediaBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(612),
                Object::Integer(792),
            ]),
        );
        set(&mut page, "Annots", Object::Array(vec![Object::Reference(widget_id)]));
        doc.objects.insert(page_id, Object::Dictionary(page));

        let mut pages = Dictionary::new();
        set(&mut pages, "Type", Object::Name(b"Pages".to_vec()));
        set(&mut pages, "Kids", Object::Array(vec![Object::Reference(page_id)]));
        set(&mut pages, "Count", Object::Integer(1));
        doc.objects.insert(pages_id, Object::Dictionary(pages));

        let mut acro = Dictionary::new();
        set(&mut acro, "Fields", Object::Array(vec![Object::Reference(top_id)]));
        set(&mut acro, "DA", Object::string_literal("/Helv 10 Tf 0 g"));
        let acro_id = doc.add_object(Object::Dictionary(acro));

        let mut catalog = Dictionary::new();
        set(&mut catalog, "Type", Object::Name(b"Catalog".to_vec()));
        set(&mut catalog, "Pages", Object::Reference(pages_id));
        set(&mut catalog, "AcroForm", Object::Reference(acro_id));
        let catalog_id = doc.add_object(Object::Dictionary(catalog));
        doc.trailer.set("Root", Object::Reference(catalog_id));

        // The name get_fields() exposes for this leaf.
        let fq = "topmostSubform[0].Page1[0].f1_07[0]".to_owned();
        let reader = PdfReaderCompat::from_document(doc);
        let fields = reader.get_fields().unwrap().unwrap();
        assert!(fields.contains_key(&fq), "get_fields keys: {:?}", fields.keys().collect::<Vec<_>>());

        let mut writer = PdfWriterCompat::from_document(reader.into_inner());
        let mut updates = BTreeMap::new();
        updates.insert(fq.clone(), FieldInput::Text("Maria Garcia".to_owned()));
        let report = writer
            .update_page_form_field_values(PageSelection::All, &updates, 0, Some(true), false)
            .unwrap();

        assert_eq!(report.unmatched, Vec::<String>::new(), "fully-qualified name must match");
        assert_eq!(report.matched, vec![fq]);
        assert_eq!(report.widgets_updated, 1);
    }

    /// A caller that drops/mangles the intermediate AcroForm group segments but
    /// keeps the correct leaf field name (a very common LLM behavior) should
    /// still fill the widget, via the leaf-`/T` fallback match.
    #[test]
    fn fills_by_leaf_when_group_path_is_mangled() {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let widget_id = doc.new_object_id();
        let page_id = doc.new_object_id();

        let mut widget = Dictionary::new();
        widget.set("Type", Object::Name(b"Annot".to_vec()));
        widget.set("Subtype", Object::Name(b"Widget".to_vec()));
        widget.set("FT", Object::Name(b"Tx".to_vec()));
        widget.set("T", Object::string_literal("f1_07[0]"));
        widget.set(
            "Rect",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(150),
                Object::Integer(20),
            ]),
        );
        doc.objects.insert(widget_id, Object::Dictionary(widget));

        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference(pages_id));
        page.set(
            "MediaBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(612),
                Object::Integer(792),
            ]),
        );
        page.set("Annots", Object::Array(vec![Object::Reference(widget_id)]));
        doc.objects.insert(page_id, Object::Dictionary(page));

        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Kids", Object::Array(vec![Object::Reference(page_id)]));
        pages.set("Count", Object::Integer(1));
        doc.objects.insert(pages_id, Object::Dictionary(pages));

        let mut acro = Dictionary::new();
        acro.set("Fields", Object::Array(vec![Object::Reference(widget_id)]));
        let acro_id = doc.add_object(Object::Dictionary(acro));
        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference(pages_id));
        catalog.set("AcroForm", Object::Reference(acro_id));
        let catalog_id = doc.add_object(Object::Dictionary(catalog));
        doc.trailer.set("Root", Object::Reference(catalog_id));

        let mut writer = PdfWriterCompat::from_document(doc);
        let mut updates = BTreeMap::new();
        // Real field is just `f1_07[0]`, but the caller invented a group path.
        let mangled = "topmostSubform[0].Page1[0].Address_ReadOrder[0].f1_07[0]".to_owned();
        updates.insert(mangled.clone(), FieldInput::Text("Mary Smith".to_owned()));
        let report = writer
            .update_page_form_field_values(PageSelection::All, &updates, 0, Some(true), false)
            .unwrap();
        assert_eq!(report.matched, vec![mangled], "leaf `/T` should match a mangled group path");
        assert!(report.unmatched.is_empty());
    }

    /// A requested name that matches no widget must be reported as `unmatched`
    /// (so integrations can fail loudly instead of treating a no-op as success).
    #[test]
    fn unmatched_field_is_reported_not_silently_dropped() {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        let widget_id = doc.new_object_id();
        let page_id = doc.new_object_id();

        let mut widget = Dictionary::new();
        widget.set("Type", Object::Name(b"Annot".to_vec()));
        widget.set("Subtype", Object::Name(b"Widget".to_vec()));
        widget.set("FT", Object::Name(b"Tx".to_vec()));
        widget.set("T", Object::string_literal("real_field"));
        widget.set(
            "Rect",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(100),
                Object::Integer(20),
            ]),
        );
        doc.objects.insert(widget_id, Object::Dictionary(widget));

        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference(pages_id));
        page.set(
            "MediaBox",
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(0),
                Object::Integer(612),
                Object::Integer(792),
            ]),
        );
        page.set("Annots", Object::Array(vec![Object::Reference(widget_id)]));
        doc.objects.insert(page_id, Object::Dictionary(page));

        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Kids", Object::Array(vec![Object::Reference(page_id)]));
        pages.set("Count", Object::Integer(1));
        doc.objects.insert(pages_id, Object::Dictionary(pages));

        let mut acro = Dictionary::new();
        acro.set("Fields", Object::Array(vec![Object::Reference(widget_id)]));
        let acro_id = doc.add_object(Object::Dictionary(acro));
        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference(pages_id));
        catalog.set("AcroForm", Object::Reference(acro_id));
        let catalog_id = doc.add_object(Object::Dictionary(catalog));
        doc.trailer.set("Root", Object::Reference(catalog_id));

        let mut writer = PdfWriterCompat::from_document(doc);
        let mut updates = BTreeMap::new();
        updates.insert("real_field".to_owned(), FieldInput::Text("ok".to_owned()));
        updates.insert("ghost_field".to_owned(), FieldInput::Text("nope".to_owned()));
        let report = writer
            .update_page_form_field_values(PageSelection::All, &updates, 0, Some(true), false)
            .unwrap();

        assert_eq!(report.matched, vec!["real_field".to_owned()]);
        assert_eq!(report.unmatched, vec!["ghost_field".to_owned()]);
        assert!(!report.all_matched());
    }
}
