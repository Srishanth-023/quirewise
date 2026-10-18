//! Writing the document outline (bookmarks).

use application::AppError;
use application::ports::OutlineEntry;
use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};

/// A PDF text string: plain bytes when ASCII, otherwise UTF-16BE with a byte order mark.
pub fn text_string(text: &str) -> Object {
    if text.is_ascii() {
        return Object::String(text.as_bytes().to_vec(), StringFormat::Literal);
    }
    let mut bytes = vec![0xFE, 0xFF];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    Object::String(bytes, StringFormat::Literal)
}

struct Node {
    title: String,
    page: ObjectId,
    parent: Option<usize>,
    children: Vec<usize>,
}

/// Replaces the outline with `entries`. Levels may skip (a level 3 under a level 1 becomes a
/// child of it) but never nest deeper than the entry before them allows.
pub fn set_outline(doc: &mut Document, entries: &[OutlineEntry]) -> Result<(), AppError> {
    let pages = doc.get_pages();
    let mut nodes: Vec<Node> = Vec::new();
    let mut stack: Vec<(u32, usize)> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();

    for entry in entries {
        let Some(&page) = pages.get(&(entry.page.get() + 1)) else {
            continue;
        };
        while stack.last().is_some_and(|(level, _)| *level >= entry.level) {
            stack.pop();
        }
        let index = nodes.len();
        let parent = stack.last().map(|(_, i)| *i);
        nodes.push(Node {
            title: entry.title.clone(),
            page,
            parent,
            children: Vec::new(),
        });
        match parent {
            Some(p) => nodes[p].children.push(index),
            None => roots.push(index),
        }
        stack.push((entry.level, index));
    }

    let catalog_id = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| AppError::internal("editing the document"))?;
    let catalog = doc
        .get_dictionary_mut(catalog_id)
        .map_err(|_| AppError::internal("editing the document"))?;
    catalog.remove(b"Outlines");
    if nodes.is_empty() {
        return Ok(());
    }

    let root_id = doc.add_object(Object::Null);
    let ids: Vec<ObjectId> = nodes.iter().map(|_| doc.add_object(Object::Null)).collect();

    let descendants = |i: usize| -> i64 {
        let mut count = 0i64;
        let mut work = nodes[i].children.clone();
        while let Some(n) = work.pop() {
            count += 1;
            work.extend(nodes[n].children.iter().copied());
        }
        count
    };

    for (i, node) in nodes.iter().enumerate() {
        let siblings: &[usize] = match node.parent {
            Some(p) => &nodes[p].children,
            None => &roots,
        };
        let position = siblings.iter().position(|s| *s == i).unwrap_or(0);
        let mut dict = Dictionary::new();
        dict.set("Title", text_string(&node.title));
        dict.set(
            "Parent",
            Object::Reference(node.parent.map_or(root_id, |p| ids[p])),
        );
        dict.set(
            "Dest",
            Object::Array(vec![
                Object::Reference(node.page),
                Object::Name(b"Fit".to_vec()),
            ]),
        );
        if position > 0 {
            dict.set("Prev", Object::Reference(ids[siblings[position - 1]]));
        }
        if let Some(next) = siblings.get(position + 1) {
            dict.set("Next", Object::Reference(ids[*next]));
        }
        if let (Some(first), Some(last)) = (node.children.first(), node.children.last()) {
            dict.set("First", Object::Reference(ids[*first]));
            dict.set("Last", Object::Reference(ids[*last]));
            dict.set("Count", Object::Integer(descendants(i)));
        }
        doc.objects.insert(ids[i], Object::Dictionary(dict));
    }

    let mut root = Dictionary::new();
    root.set("Type", Object::Name(b"Outlines".to_vec()));
    if let (Some(first), Some(last)) = (roots.first(), roots.last()) {
        root.set("First", Object::Reference(ids[*first]));
        root.set("Last", Object::Reference(ids[*last]));
    }
    root.set("Count", Object::Integer(nodes.len() as i64));
    doc.objects.insert(root_id, Object::Dictionary(root));

    doc.get_dictionary_mut(catalog_id)
        .map_err(|_| AppError::internal("editing the document"))?
        .set("Outlines", Object::Reference(root_id));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_titles_are_plain_and_others_are_utf16() {
        assert_eq!(
            text_string("Intro"),
            Object::String(b"Intro".to_vec(), StringFormat::Literal)
        );
        let Object::String(bytes, _) = text_string("Café") else {
            panic!("expected a string");
        };
        assert_eq!(&bytes[..2], &[0xFE, 0xFF]);
        assert_eq!(bytes.len(), 2 + 4 * 2);
    }
}
