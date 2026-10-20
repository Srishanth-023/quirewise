//! An sRGB color profile, made here: PDF/A needs a profile in the file that says what the colors
//! of device RGB and gray mean, and a profile written from the published sRGB numbers carries no
//! license. It is a version 2 matrix and curve profile, which every reader understands.

/// s15Fixed16 number: a value times 65536, rounded.
fn fixed(value: f64) -> [u8; 4] {
    ((value * 65536.0).round() as i32).to_be_bytes()
}

fn xyz_tag(x: f64, y: f64, z: f64) -> Vec<u8> {
    let mut tag = b"XYZ \0\0\0\0".to_vec();
    for v in [x, y, z] {
        tag.extend_from_slice(&fixed(v));
    }
    tag
}

/// The sRGB transfer curve as 1024 samples.
fn curve_tag() -> Vec<u8> {
    let mut tag = b"curv\0\0\0\0".to_vec();
    tag.extend_from_slice(&1024u32.to_be_bytes());
    for i in 0..1024u32 {
        let v = f64::from(i) / 1023.0;
        let linear = if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        };
        tag.extend_from_slice(&((linear * 65535.0).round() as u16).to_be_bytes());
    }
    tag
}

fn text_tag(text: &str) -> Vec<u8> {
    let mut tag = b"text\0\0\0\0".to_vec();
    tag.extend_from_slice(text.as_bytes());
    tag.push(0);
    tag
}

fn description_tag(text: &str) -> Vec<u8> {
    let mut tag = b"desc\0\0\0\0".to_vec();
    tag.extend_from_slice(&(text.len() as u32 + 1).to_be_bytes());
    tag.extend_from_slice(text.as_bytes());
    tag.push(0);
    // No Unicode text and no Macintosh script text, as the format asks.
    tag.extend_from_slice(&[0; 4 + 4 + 2 + 1 + 67]);
    tag
}

fn pad4(data: &mut Vec<u8>) {
    while !data.len().is_multiple_of(4) {
        data.push(0);
    }
}

/// The bytes of an sRGB profile.
pub fn srgb() -> Vec<u8> {
    let curve = curve_tag();
    // (signature, data); the three curves share one block of data.
    let mut blocks: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"desc", description_tag("sRGB IEC61966-2.1")),
        (*b"cprt", text_tag("No copyright, use freely")),
        (*b"wtpt", xyz_tag(0.9642, 1.0, 0.8249)),
        (*b"rXYZ", xyz_tag(0.43607, 0.22249, 0.01392)),
        (*b"gXYZ", xyz_tag(0.38515, 0.71687, 0.09708)),
        (*b"bXYZ", xyz_tag(0.14307, 0.06061, 0.71410)),
    ];
    for (_, data) in &mut blocks {
        pad4(data);
    }
    let table_size = 4 + 12 * (blocks.len() + 3);
    let mut offset = 128 + table_size;
    let mut table = Vec::new();
    table.extend_from_slice(&((blocks.len() + 3) as u32).to_be_bytes());
    let mut body = Vec::new();
    for (signature, data) in &blocks {
        table.extend_from_slice(signature);
        table.extend_from_slice(&(offset as u32).to_be_bytes());
        table.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    // The three curves are one tag body that three entries point at.
    for signature in [b"rTRC", b"gTRC", b"bTRC"] {
        table.extend_from_slice(signature);
        table.extend_from_slice(&(offset as u32).to_be_bytes());
        table.extend_from_slice(&(curve.len() as u32).to_be_bytes());
    }
    body.extend_from_slice(&curve);

    let size = 128 + table.len() + body.len();
    let mut header = vec![0u8; 128];
    header[0..4].copy_from_slice(&(size as u32).to_be_bytes());
    header[8..12].copy_from_slice(&[2, 0x10, 0, 0]);
    header[12..16].copy_from_slice(b"mntr");
    header[16..20].copy_from_slice(b"RGB ");
    header[20..24].copy_from_slice(b"XYZ ");
    // 2009-01-01 00:00:00
    for (i, v) in [2009u16, 1, 1, 0, 0, 0].into_iter().enumerate() {
        header[24 + 2 * i..26 + 2 * i].copy_from_slice(&v.to_be_bytes());
    }
    header[36..40].copy_from_slice(b"acsp");
    header[40..44].copy_from_slice(b"MSFT");
    // The illuminant of the connection space is D50.
    header[68..72].copy_from_slice(&fixed(0.9642));
    header[72..76].copy_from_slice(&fixed(1.0));
    header[76..80].copy_from_slice(&fixed(0.8249));
    let mut profile = header;
    profile.extend_from_slice(&table);
    profile.extend_from_slice(&body);
    profile
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be32(bytes: &[u8], at: usize) -> u32 {
        u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn the_header_describes_a_version_2_monitor_profile() {
        let p = srgb();
        assert_eq!(be32(&p, 0) as usize, p.len(), "the size field is the size");
        assert_eq!(&p[8..12], &[2, 0x10, 0, 0]);
        assert_eq!(&p[12..16], b"mntr");
        assert_eq!(&p[16..20], b"RGB ");
        assert_eq!(&p[20..24], b"XYZ ");
        assert_eq!(&p[36..40], b"acsp");
        assert_eq!(p.len() % 4, 0);
    }

    #[test]
    fn every_tag_lies_inside_the_profile_and_is_aligned() {
        let p = srgb();
        let count = be32(&p, 128) as usize;
        assert_eq!(count, 9);
        let mut signatures = Vec::new();
        for i in 0..count {
            let at = 132 + 12 * i;
            let (offset, size) = (be32(&p, at + 4) as usize, be32(&p, at + 8) as usize);
            assert_eq!(offset % 4, 0, "tag {i} is aligned");
            assert!(offset + size <= p.len(), "tag {i} fits");
            signatures.push(String::from_utf8_lossy(&p[at..at + 4]).into_owned());
        }
        for needed in [
            "desc", "cprt", "wtpt", "rXYZ", "gXYZ", "bXYZ", "rTRC", "gTRC", "bTRC",
        ] {
            assert!(signatures.iter().any(|s| s == needed), "{needed}");
        }
    }

    #[test]
    fn the_curve_runs_from_black_to_white_and_rises() {
        let curve = curve_tag();
        let values: Vec<u16> = curve[12..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        assert_eq!(values.len(), 1024);
        assert_eq!(values[0], 0);
        assert_eq!(values[1023], 65535);
        assert!(values.windows(2).all(|w| w[0] <= w[1]));
        // Mid gray 0.5 encodes to about 21.4 percent light.
        let mid = f64::from(values[512]) / 65535.0;
        assert!((mid - 0.2140).abs() < 0.005, "{mid}");
    }

    #[test]
    fn the_primaries_add_up_to_the_white_point() {
        let x: f64 = [0.43607, 0.38515, 0.14307].iter().sum();
        let y: f64 = [0.22249, 0.71687, 0.06061].iter().sum();
        let z: f64 = [0.01392, 0.09708, 0.71410].iter().sum();
        assert!(
            (x - 0.9642).abs() < 0.002 && (y - 1.0).abs() < 0.002 && (z - 0.8249).abs() < 0.002
        );
    }
}
