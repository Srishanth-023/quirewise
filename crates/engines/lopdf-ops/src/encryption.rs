//! Reads the Encrypt dictionary: cipher, key length, revision and the permission bits.

use application::inspect::{EncryptionInfo, Permissions};
use lopdf::{Dictionary, Document, Object};

fn int(dict: &Dictionary, key: &[u8]) -> Option<i64> {
    dict.get(key).ok().and_then(|o| o.as_i64().ok())
}

/// The Encrypt dictionary, whether the trailer holds it directly or by reference.
fn encrypt_dict(doc: &Document) -> Option<&Dictionary> {
    let object = doc.trailer.get(b"Encrypt").ok()?;
    let (_, resolved) = doc.dereference(object).ok()?;
    resolved.as_dict().ok()
}

/// Name of the crypt filter method used by the standard crypt filter, for V4 and V5.
fn crypt_filter_method(dict: &Dictionary) -> Option<Vec<u8>> {
    let cf = dict.get(b"CF").ok()?.as_dict().ok()?;
    let std_cf = cf.get(b"StdCF").ok()?.as_dict().ok()?;
    match std_cf.get(b"CFM").ok()? {
        Object::Name(name) => Some(name.clone()),
        _ => None,
    }
}

/// Describes the encryption of a loaded document, or `None` when it is not encrypted.
pub fn describe(doc: &Document) -> Option<(EncryptionInfo, Option<Permissions>)> {
    let dict = encrypt_dict(doc)?;
    let v = int(dict, b"V").unwrap_or(0);
    let revision = int(dict, b"R").and_then(|r| u32::try_from(r).ok());
    let length = int(dict, b"Length").and_then(|l| u32::try_from(l).ok());

    let (method, key_bits) = match v {
        1 => ("RC4".to_owned(), Some(40)),
        2 | 3 => ("RC4".to_owned(), length.or(Some(128))),
        4 => match crypt_filter_method(dict).as_deref() {
            Some(b"AESV2") => ("AES-128".to_owned(), Some(128)),
            Some(b"V2") => ("RC4".to_owned(), Some(128)),
            _ => ("Unknown".to_owned(), None),
        },
        5 => ("AES-256".to_owned(), Some(256)),
        _ => ("Unknown".to_owned(), None),
    };

    let permissions = int(dict, b"P").map(|p| permissions_from_bits(p as i32 as u32));
    Some((
        EncryptionInfo {
            method,
            key_bits,
            revision,
        },
        permissions,
    ))
}

/// Decodes the `/P` permission bits (PDF 32000-1 table 22).
pub fn permissions_from_bits(p: u32) -> Permissions {
    let bit = |n: u32| p & (1 << (n - 1)) != 0;
    Permissions {
        print: bit(3),
        modify: bit(4),
        copy: bit(5),
        annotate: bit(6),
        fill_forms: bit(9),
        assemble: bit(11),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_bits_follow_the_spec() {
        let all = permissions_from_bits(0xFFFF_FFFC);
        assert!(
            all.print && all.modify && all.copy && all.annotate && all.fill_forms && all.assemble
        );
        let none = permissions_from_bits(0xFFFF_F0C0);
        assert!(!none.print && !none.modify && !none.copy && !none.annotate);
        assert!(!none.fill_forms && !none.assemble);
        let print_only = permissions_from_bits(0xFFFF_F0C4);
        assert!(print_only.print && !print_only.copy);
    }
}
