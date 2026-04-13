//! PDF document operations: merge, split, rotate, encrypt/decrypt.

use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
use lopdf::{Document, EncryptionVersion, Object, ObjectId, Permissions};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use crate::{PdferError, Result};

/// Merge multiple PDF documents into a single document.
///
/// Takes ownership of each source `Document`, renumbers their object IDs to avoid
/// collisions, then builds a unified page tree containing all pages in order.
pub fn merge_documents(documents: Vec<Document>) -> Result<Document> {
    if documents.is_empty() {
        return Err(PdferError::Message("no documents to merge".into()));
    }

    let mut merged = Document::with_version("1.5");
    let mut max_id: u32 = 1;

    // Collected page object IDs (in order) and their dictionaries.
    let mut all_page_ids: Vec<ObjectId> = Vec::new();

    // Non-page, non-catalog, non-pages-tree objects collected from each source doc.
    let mut documents_pages: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut documents_objects: BTreeMap<ObjectId, Object> = BTreeMap::new();

    let mut catalog_object: Option<(ObjectId, Object)> = None;
    let mut pages_object: Option<(ObjectId, Object)> = None;

    for mut doc in documents {
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;

        // Collect page IDs from this document (preserving order).
        let page_ids: Vec<ObjectId> = doc
            .get_pages()
            .into_iter()
            .map(|(_, object_id)| object_id)
            .collect();

        // Store each page object separately so we can reparent later.
        for &page_id in &page_ids {
            if let Ok(obj) = doc.get_object(page_id) {
                documents_pages.insert(page_id, obj.clone());
            }
        }

        all_page_ids.extend(page_ids);

        // Move all objects into our working collection.
        for (id, obj) in doc.objects {
            documents_objects.insert(id, obj);
        }
    }

    // Classify objects: pull out Catalog and Pages trees, insert everything else.
    for (object_id, object) in documents_objects.iter() {
        match object.type_name().unwrap_or(b"") {
            b"Catalog" => {
                catalog_object = Some((
                    catalog_object.map(|(id, _)| id).unwrap_or(*object_id),
                    object.clone(),
                ));
            }
            b"Pages" => {
                if let Ok(dictionary) = object.as_dict() {
                    let mut dictionary = dictionary.clone();
                    if let Some((_, ref existing)) = pages_object {
                        if let Ok(old_dict) = existing.as_dict() {
                            dictionary.extend(old_dict);
                        }
                    }
                    pages_object = Some((
                        pages_object.map(|(id, _)| id).unwrap_or(*object_id),
                        Object::Dictionary(dictionary),
                    ));
                }
            }
            b"Page" | b"Outlines" | b"Outline" => {
                // Handled separately.
            }
            _ => {
                merged.objects.insert(*object_id, object.clone());
            }
        }
    }

    let pages_object =
        pages_object.ok_or_else(|| PdferError::Message("no Pages object found".into()))?;
    let catalog_object =
        catalog_object.ok_or_else(|| PdferError::Message("no Catalog object found".into()))?;

    // Insert page objects with updated /Parent.
    for (object_id, object) in &documents_pages {
        if let Ok(dictionary) = object.as_dict() {
            let mut dictionary = dictionary.clone();
            dictionary.set("Parent", pages_object.0);
            merged
                .objects
                .insert(*object_id, Object::Dictionary(dictionary));
        }
    }

    // Build the unified /Pages dictionary.
    if let Ok(dictionary) = pages_object.1.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Count", all_page_ids.len() as u32);
        dictionary.set(
            "Kids",
            all_page_ids
                .iter()
                .map(|&id| Object::Reference(id))
                .collect::<Vec<_>>(),
        );
        merged
            .objects
            .insert(pages_object.0, Object::Dictionary(dictionary));
    }

    // Build the unified /Catalog.
    if let Ok(dictionary) = catalog_object.1.as_dict() {
        let mut dictionary = dictionary.clone();
        dictionary.set("Pages", pages_object.0);
        dictionary.remove(b"Outlines");
        merged
            .objects
            .insert(catalog_object.0, Object::Dictionary(dictionary));
    }

    merged.trailer.set("Root", catalog_object.0);
    merged.max_id = merged.objects.len() as u32;

    merged.renumber_objects();
    merged.prune_objects();

    Ok(merged)
}

/// Convenience wrapper that loads PDF files from disk and merges them.
pub fn merge_files(paths: &[impl AsRef<Path>]) -> Result<Document> {
    let documents: Result<Vec<Document>> = paths
        .iter()
        .map(|p| Document::load(p.as_ref()).map_err(PdferError::from))
        .collect();
    merge_documents(documents?)
}

/// Extract specific pages from a document into a new document.
///
/// `page_numbers` are 1-based, matching the keys returned by `Document::get_pages()`.
/// Pages not listed are removed; the original document is not modified.
pub fn split_pages(document: &Document, page_numbers: &[u32]) -> Result<Document> {
    if page_numbers.is_empty() {
        return Err(PdferError::Message("no page numbers specified".into()));
    }

    let mut doc = document.clone();
    let all_pages = doc.get_pages();

    // Validate that all requested pages exist.
    for &pn in page_numbers {
        if !all_pages.contains_key(&pn) {
            return Err(PdferError::MissingPage(pn as usize));
        }
    }

    // Determine which pages to delete (those NOT in the requested set).
    let requested: std::collections::BTreeSet<u32> = page_numbers.iter().copied().collect();
    let to_delete: Vec<u32> = all_pages
        .keys()
        .filter(|k| !requested.contains(k))
        .copied()
        .collect();

    doc.delete_pages(&to_delete);
    doc.prune_objects();

    Ok(doc)
}

/// Split a document into one document per page.
///
/// Returns a `Vec<Document>` where each element contains exactly one page
/// from the original, preserving page order.
pub fn split_each_page(document: &Document) -> Result<Vec<Document>> {
    let pages = document.get_pages();
    if pages.is_empty() {
        return Err(PdferError::Message("document has no pages".into()));
    }

    let mut result = Vec::with_capacity(pages.len());
    for &page_num in pages.keys() {
        result.push(split_pages(document, &[page_num])?);
    }
    Ok(result)
}

/// Rotate specified pages by the given number of degrees.
///
/// `page_numbers` are 1-based. `degrees` must be 0, 90, 180, or 270.
/// The `/Rotate` entry is set on each matching page dictionary.
pub fn rotate_pages(document: &mut Document, page_numbers: &[u32], degrees: i64) -> Result<()> {
    if !matches!(degrees, 0 | 90 | 180 | 270) {
        return Err(PdferError::Message(format!(
            "invalid rotation degrees {degrees}: must be 0, 90, 180, or 270"
        )));
    }

    let pages = document.get_pages();

    for &pn in page_numbers {
        let &page_id = pages
            .get(&pn)
            .ok_or(PdferError::MissingPage(pn as usize))?;

        let page_obj = document
            .get_object_mut(page_id)
            .map_err(|_| PdferError::MissingPage(pn as usize))?;

        if let Ok(dict) = page_obj.as_dict_mut() {
            dict.set("Rotate", Object::Integer(degrees));
        } else {
            return Err(PdferError::InvalidStructure("page object is not a dictionary"));
        }
    }

    Ok(())
}

/// Encrypt a PDF document with user and owner passwords using AES-128 (V4/R4).
///
/// Uses lopdf's built-in encryption support. The V4 scheme (AES-128, revision 4)
/// is used because it derives the file encryption key internally from the passwords
/// and document ID, requiring no external random key generation.
///
/// All permissions (print, copy, modify, etc.) are granted by default.
pub fn encrypt_document(
    document: &mut Document,
    user_password: &str,
    owner_password: &str,
) -> Result<()> {
    let mut crypt_filters: BTreeMap<Vec<u8>, Arc<dyn CryptFilter>> = BTreeMap::new();
    crypt_filters.insert(
        b"StdCF".to_vec(),
        Arc::new(Aes128CryptFilter) as Arc<dyn CryptFilter>,
    );

    let version = EncryptionVersion::V4 {
        document: &*document,
        encrypt_metadata: true,
        crypt_filters,
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password,
        user_password,
        permissions: Permissions::default(),
    };

    let state = lopdf::EncryptionState::try_from(version)?;
    document.encrypt(&state)?;

    Ok(())
}

/// Decrypt an encrypted PDF document using the given password.
///
/// Wraps `Document::decrypt()`, which handles both user and owner password
/// authentication internally.
pub fn decrypt_document(document: &mut Document, password: &str) -> Result<()> {
    document.decrypt(password)?;
    Ok(())
}
