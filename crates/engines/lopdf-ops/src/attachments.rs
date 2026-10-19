//! File attachments: the files embedded in a document and listed under `Names/EmbeddedFiles`.

use application::AppError;
use application::ports::{Attachment, AttachmentInfo};
use domain::ErrorCode;
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, StringFormat};
use std::path::PathBuf;

use crate::outline::text_string;

/// Largest single attachment read or added.
const MAX_ATTACHMENT_BYTES: u64 = 256 * 1024 * 1024;
/// Deepest name tree followed, so a cyclic tree in a hostile file ends.
const MAX_TREE_DEPTH: usize = 16;

/// One entry of the name tree: the name and the object that describes the file.
#[derive(Debug, Clone)]
struct Entry {
    name: Vec<u8>,
    spec: Object,
}

fn resolve_dict<'a>(doc: &'a Document, object: &'a Object) -> Option<&'a Dictionary> {
    match object {
        Object::Dictionary(d) => Some(d),
        Object::Reference(id) => doc.get_dictionary(*id).ok(),
        _ => None,
    }
}

fn walk(doc: &Document, node: &Dictionary, depth: usize, out: &mut Vec<Entry>) {
    if depth > MAX_TREE_DEPTH {
        return;
    }
    if let Ok(Object::Array(names)) = node.get(b"Names") {
        for pair in names.as_chunks::<2>().0 {
            let name = match &pair[0] {
                Object::String(bytes, _) => bytes.clone(),
                _ => continue,
            };
            out.push(Entry {
                name,
                spec: pair[1].clone(),
            });
        }
    }
    if let Ok(Object::Array(kids)) = node.get(b"Kids") {
        for kid in kids {
            if let Some(child) = resolve_dict(doc, kid) {
                walk(doc, child, depth + 1, out);
            }
        }
    }
}

fn catalog_id(doc: &Document) -> Result<ObjectId, AppError> {
    doc.trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| AppError::internal("editing the document"))
}

fn entries(doc: &Document) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(catalog) = doc.catalog() else {
        return out;
    };
    let Some(names) = catalog
        .get(b"Names")
        .ok()
        .and_then(|o| resolve_dict(doc, o))
    else {
        return out;
    };
    let Some(tree) = names
        .get(b"EmbeddedFiles")
        .ok()
        .and_then(|o| resolve_dict(doc, o))
    else {
        return out;
    };
    walk(doc, tree, 0, &mut out);
    out
}

/// PDF text string to text.
fn name_text(bytes: &[u8]) -> String {
    crate::metadata::read_text(&Object::String(bytes.to_vec(), StringFormat::Literal))
        .unwrap_or_default()
}

/// The embedded file stream of a file specification.
fn stream_of<'a>(doc: &'a Document, spec: &'a Object) -> Option<&'a Stream> {
    let dict = resolve_dict(doc, spec)?;
    let files = resolve_dict(doc, dict.get(b"EF").ok()?)?;
    let target = files.get(b"UF").or_else(|_| files.get(b"F")).ok()?;
    match target {
        Object::Reference(id) => doc.get_object(*id).ok()?.as_stream().ok(),
        Object::Stream(s) => Some(s),
        _ => None,
    }
}

/// Lists the attachments of a document.
pub fn list(doc: &Document) -> Vec<AttachmentInfo> {
    entries(doc)
        .iter()
        .map(|e| {
            let dict = resolve_dict(doc, &e.spec);
            let description = dict
                .and_then(|d| d.get(b"Desc").ok())
                .and_then(crate::metadata::read_text);
            let size = stream_of(doc, &e.spec).and_then(|s| {
                s.dict
                    .get(b"Params")
                    .ok()
                    .and_then(|p| resolve_dict(doc, p))
                    .and_then(|p| p.get(b"Size").ok())
                    .and_then(|v| v.as_i64().ok())
                    .and_then(|v| u64::try_from(v).ok())
                    .or_else(|| u64::try_from(s.content.len()).ok())
            });
            AttachmentInfo {
                name: name_text(&e.name),
                size,
                description,
            }
        })
        .collect()
}

/// The content of the attachments called `names`, or of all when `names` is empty.
pub fn read(doc: &Document, names: &[String]) -> Result<Vec<Attachment>, AppError> {
    let mut out = Vec::new();
    for entry in entries(doc) {
        let name = name_text(&entry.name);
        if !names.is_empty() && !names.contains(&name) {
            continue;
        }
        let Some(stream) = stream_of(doc, &entry.spec) else {
            continue;
        };
        let data = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        if data.len() as u64 > MAX_ATTACHMENT_BYTES {
            return Err(AppError::new(
                ErrorCode::PdfLimitExceeded,
                "An attachment is larger than 256 MB and was not read.",
            ));
        }
        out.push(Attachment { name, data });
    }
    for wanted in names {
        if !out.iter().any(|a| a.name == *wanted) {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                format!("The document has no attachment named \"{wanted}\"."),
            ));
        }
    }
    Ok(out)
}

fn store(doc: &mut Document, mut all: Vec<Entry>) -> Result<(), AppError> {
    let id = catalog_id(doc)?;
    // Name trees are sorted by name.
    all.sort_by(|a, b| a.name.cmp(&b.name));
    let names_id = match doc
        .get_dictionary(id)
        .ok()
        .and_then(|c| c.get(b"Names").ok())
    {
        Some(Object::Reference(r)) => Some(*r),
        _ => None,
    };
    let mut names_dict = {
        let catalog = doc
            .get_dictionary(id)
            .map_err(|_| AppError::internal("editing the document"))?;
        catalog
            .get(b"Names")
            .ok()
            .and_then(|o| resolve_dict(doc, o))
            .cloned()
            .unwrap_or_default()
    };
    if all.is_empty() {
        names_dict.remove(b"EmbeddedFiles");
    } else {
        let mut flat = Vec::with_capacity(all.len() * 2);
        for e in all {
            flat.push(Object::String(e.name, StringFormat::Literal));
            flat.push(e.spec);
        }
        let mut tree = Dictionary::new();
        tree.set("Names", Object::Array(flat));
        names_dict.set("EmbeddedFiles", Object::Dictionary(tree));
    }
    match names_id {
        Some(r) => {
            doc.objects.insert(r, Object::Dictionary(names_dict));
        }
        None => {
            let catalog = doc
                .get_dictionary_mut(id)
                .map_err(|_| AppError::internal("editing the document"))?;
            if names_dict.is_empty() {
                catalog.remove(b"Names");
            } else {
                catalog.set("Names", Object::Dictionary(names_dict));
            }
        }
    }
    Ok(())
}

/// A name that no entry of `taken` has yet: `a.txt`, then `a (2).txt`, `a (3).txt`.
fn unique_name(taken: &[Entry], wanted: &str) -> String {
    let used = |candidate: &str| taken.iter().any(|e| name_text(&e.name) == candidate);
    if !used(wanted) {
        return wanted.to_owned();
    }
    let (stem, ext) = match wanted.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_owned(), format!(".{e}")),
        _ => (wanted.to_owned(), String::new()),
    };
    (2u32..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|c| !used(c))
        .unwrap_or_else(|| wanted.to_owned())
}

/// Attaches the files at `paths`.
pub fn add(doc: &mut Document, paths: &[PathBuf]) -> Result<(), AppError> {
    let mut all = entries(doc);
    for path in paths {
        let meta =
            std::fs::metadata(path).map_err(|e| AppError::from_io("read the attachment", &e))?;
        if !meta.is_file() || meta.len() > MAX_ATTACHMENT_BYTES {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                "An attachment must be a file of at most 256 MB.",
            ));
        }
        let data = std::fs::read(path).map_err(|e| AppError::from_io("read the attachment", &e))?;
        let wanted = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "attachment".to_owned());
        let name = unique_name(&all, &wanted);
        let mut params = Dictionary::new();
        params.set(
            "Size",
            Object::Integer(i64::try_from(data.len()).unwrap_or(i64::MAX)),
        );
        let mut stream_dict = Dictionary::new();
        stream_dict.set("Type", Object::Name(b"EmbeddedFile".to_vec()));
        stream_dict.set("Params", Object::Dictionary(params));
        let stream = doc.add_object(Stream::new(stream_dict, data));
        let mut files = Dictionary::new();
        files.set("F", Object::Reference(stream));
        files.set("UF", Object::Reference(stream));
        let mut spec = Dictionary::new();
        spec.set("Type", Object::Name(b"Filespec".to_vec()));
        spec.set("F", text_string(&name));
        spec.set("UF", text_string(&name));
        spec.set("EF", Object::Dictionary(files));
        let spec_id = doc.add_object(Object::Dictionary(spec));
        let key = match text_string(&name) {
            Object::String(bytes, _) => bytes,
            _ => name.clone().into_bytes(),
        };
        all.push(Entry {
            name: key,
            spec: Object::Reference(spec_id),
        });
    }
    store(doc, all)
}

/// Removes the attachments called `names`.
pub fn remove(doc: &mut Document, names: &[String]) -> Result<(), AppError> {
    let all = entries(doc);
    for wanted in names {
        if !all.iter().any(|e| name_text(&e.name) == *wanted) {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                format!("The document has no attachment named \"{wanted}\"."),
            ));
        }
    }
    let kept: Vec<Entry> = all
        .into_iter()
        .filter(|e| !names.iter().any(|n| *n == name_text(&e.name)))
        .collect();
    store(doc, kept)?;
    doc.prune_objects();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_support::minimal_pdf;

    fn write(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_document_without_attachments_lists_none() {
        let doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        assert!(list(&doc).is_empty());
        assert!(read(&doc, &[]).unwrap().is_empty());
    }

    #[test]
    fn added_files_are_listed_with_their_sizes_and_read_back_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "notes.txt", b"hello attachments");
        let b = write(&dir, "data.csv", b"1,2,3\n");
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        add(&mut doc, &[a, b]).unwrap();
        let names: Vec<(String, Option<u64>)> =
            list(&doc).into_iter().map(|i| (i.name, i.size)).collect();
        // The name tree is sorted by name.
        assert_eq!(
            names,
            vec![
                ("data.csv".to_owned(), Some(6)),
                ("notes.txt".to_owned(), Some(17))
            ]
        );
        let all = read(&doc, &[]).unwrap();
        assert_eq!(all[1].data, b"hello attachments");
        let one = read(&doc, &["data.csv".to_owned()]).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].data, b"1,2,3\n");
    }

    #[test]
    fn a_second_file_with_the_same_name_gets_a_number() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.txt", b"one");
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let again = sub.join("a.txt");
        std::fs::write(&again, b"two").unwrap();
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        add(&mut doc, &[a, again]).unwrap();
        let names: Vec<String> = list(&doc).into_iter().map(|i| i.name).collect();
        assert_eq!(names, vec!["a (2).txt".to_owned(), "a.txt".to_owned()]);
    }

    #[test]
    fn removing_attachments_keeps_the_rest_and_drops_the_data() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.txt", b"AAAA-unique-marker");
        let b = write(&dir, "b.txt", b"keep me");
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        add(&mut doc, &[a, b]).unwrap();
        remove(&mut doc, &["a.txt".to_owned()]).unwrap();
        let names: Vec<String> = list(&doc).into_iter().map(|i| i.name).collect();
        assert_eq!(names, vec!["b.txt".to_owned()]);
        let gone = doc.objects.values().any(
            |o| matches!(o, Object::Stream(s) if s.content.windows(6).any(|w| w == b"AAAA-u")),
        );
        assert!(!gone, "the removed file's bytes are pruned");
        remove(&mut doc, &["b.txt".to_owned()]).unwrap();
        assert!(list(&doc).is_empty());
        assert!(
            doc.catalog().unwrap().get(b"Names").is_err(),
            "an empty tree leaves no Names entry"
        );
    }

    #[test]
    fn unknown_names_and_bad_paths_are_option_errors() {
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        let err = remove(&mut doc, &["x.txt".to_owned()]).unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
        let err = read(&doc, &["x.txt".to_owned()]).unwrap_err();
        assert_eq!(err.code, ErrorCode::OptionInvalid);
        let err = add(&mut doc, &[PathBuf::from("Z:\\no\\such\\file.txt")]).unwrap_err();
        assert_eq!(err.code, ErrorCode::PathDenied);
        let dir = tempfile::tempdir().unwrap();
        let err = add(&mut doc, &[dir.path().to_path_buf()]).unwrap_err();
        assert!(matches!(
            err.code,
            ErrorCode::OptionInvalid | ErrorCode::PathDenied
        ));
    }

    #[test]
    fn attachments_added_next_to_existing_names_keep_the_other_name_entries() {
        let dir = tempfile::tempdir().unwrap();
        let a = write(&dir, "a.txt", b"x");
        let mut doc = Document::load_mem(&minimal_pdf(1)).unwrap();
        let catalog_id = catalog_id(&doc).unwrap();
        let mut names = Dictionary::new();
        names.set("JavaScript", Object::Null);
        doc.get_dictionary_mut(catalog_id)
            .unwrap()
            .set("Names", Object::Dictionary(names));
        add(&mut doc, &[a]).unwrap();
        let catalog = doc.catalog().unwrap();
        let names = catalog.get(b"Names").unwrap().as_dict().unwrap();
        assert!(names.get(b"JavaScript").is_ok());
        assert!(names.get(b"EmbeddedFiles").is_ok());
    }
}
