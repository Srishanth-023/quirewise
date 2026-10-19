//! On-demand components (ADR-0005): the signed component list, downloads, offline packages and
//! the installed folder `components\<id>\<version>\`.
//!
//! Nothing is installed before its SHA-256 matches the signed list. The list itself, and the
//! entry inside an offline `.qwpkg` file, are checked against a minisign (Ed25519) public key that
//! ships with the app.

use application::AppError;
use application::components::{MAX_MANIFEST_BYTES, parse_component_manifest};
use application::ports::{ComponentInstaller, ComponentStore, Downloader};
use application::runtime::{ProgressEvent, ProgressSink};
use domain::ErrorCode;
use domain::components::{Arch, ComponentEntry};
use minisign_verify::{PublicKey, Signature};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// File name of the record written next to an installed component.
pub const RECORD_FILE: &str = "component.json";
/// Folder under the components folder where downloads wait while they are checked.
const DOWNLOAD_DIR: &str = ".download";
/// Most files one component may unpack to.
const MAX_FILES: usize = 200_000;
/// Most bytes one component may unpack to (10 GiB).
const MAX_UNPACKED: u64 = 10 * 1024 * 1024 * 1024;
/// Largest signature file.
const MAX_SIGNATURE_BYTES: u64 = 4096;
/// Largest `entry.json` in an offline package.
const MAX_ENTRY_BYTES: u64 = 64 * 1024;

fn verify_failed(why: &str) -> AppError {
    AppError::new(
        ErrorCode::ComponentVerifyFailed,
        format!("The component could not be verified ({why}). Nothing was installed."),
    )
}

fn io_error(what: &str, e: &io::Error) -> AppError {
    AppError::from_io(what, e)
}

/// Orders version strings by their numeric parts: `24.8.10` is newer than `24.8.9`.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| -> Vec<(u64, String)> {
        v.split(['.', '-', '+'])
            .map(|p| (p.parse::<u64>().unwrap_or(0), p.to_owned()))
            .collect()
    };
    let (pa, pb) = (parts(a), parts(b));
    for (x, y) in pa.iter().zip(pb.iter()) {
        let order = x.0.cmp(&y.0).then_with(|| {
            if x.0 == 0 && y.0 == 0 {
                x.1.cmp(&y.1)
            } else {
                Ordering::Equal
            }
        });
        if order != Ordering::Equal {
            return order;
        }
    }
    pa.len().cmp(&pb.len())
}

/// Component store over the components folder.
#[derive(Debug, Clone)]
pub struct FsComponentStore {
    root: PathBuf,
}

impl FsComponentStore {
    /// Creates a store over `root` (the `components` folder).
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// The folder of one installed version.
    fn version_dir(&self, entry: &ComponentEntry) -> PathBuf {
        self.root.join(&entry.id).join(&entry.version)
    }
}

impl ComponentStore for FsComponentStore {
    fn installed(&self) -> Result<Vec<ComponentEntry>, AppError> {
        let Ok(ids) = fs::read_dir(&self.root) else {
            return Ok(Vec::new());
        };
        let mut found = Vec::new();
        for id_dir in ids
            .flatten()
            .filter(|e| e.path().is_dir() && !e.file_name().to_string_lossy().starts_with('.'))
        {
            let Ok(versions) = fs::read_dir(id_dir.path()) else {
                continue;
            };
            for version_dir in versions.flatten().filter(|e| e.path().is_dir()) {
                let record = version_dir.path().join(RECORD_FILE);
                // A missing or invalid record means the install is incomplete; skip it.
                let Ok(bytes) = fs::read(&record) else {
                    continue;
                };
                if let Ok(entry) = serde_json::from_slice::<ComponentEntry>(&bytes)
                    && entry.validate().is_ok()
                {
                    found.push(entry);
                }
            }
        }
        Ok(found)
    }

    fn locate(&self, id: &str) -> Option<PathBuf> {
        let mut entries: Vec<ComponentEntry> = self
            .installed()
            .ok()?
            .into_iter()
            .filter(|e| e.id == id)
            .collect();
        entries.sort_by(|a, b| compare_versions(&a.version, &b.version));
        entries.last().map(|e| self.version_dir(e))
    }
}

/// Where the component list is published until the owner sets the real address.
pub const DEFAULT_MANIFEST_URL: &str = "https://components.invalid/v1/components.json";

/// The trust root of this build: the list address and the signing key in `trust/components.pub`.
pub fn default_trust() -> ComponentTrust {
    ComponentTrust {
        manifest_url: DEFAULT_MANIFEST_URL.to_owned(),
        public_key: include_str!("../trust/components.pub").trim().to_owned(),
    }
}

/// Where the component list lives and who signed it.
#[derive(Debug, Clone)]
pub struct ComponentTrust {
    /// The component list, `components.json`. Its signature is next to it with `.minisig` added.
    pub manifest_url: String,
    /// The minisign public key, as the base64 line of a `.pub` file.
    pub public_key: String,
}

/// Downloads, verifies and installs components.
pub struct ComponentManager {
    store: FsComponentStore,
    root: PathBuf,
    downloader: Arc<dyn Downloader>,
    trust: ComponentTrust,
    arch: Arch,
}

impl ComponentManager {
    /// Creates a manager for the components folder `root`, for a machine of architecture `arch`.
    pub fn new(
        root: PathBuf,
        downloader: Arc<dyn Downloader>,
        trust: ComponentTrust,
        arch: Arch,
    ) -> Self {
        Self {
            store: FsComponentStore::new(root.clone()),
            root,
            downloader,
            trust,
            arch,
        }
    }

    /// The architecture of this build.
    pub const fn current_arch() -> Arch {
        if cfg!(target_arch = "aarch64") {
            Arch::Arm64
        } else {
            Arch::X64
        }
    }

    fn verify_signature(&self, data: &[u8], signature: &str) -> Result<(), AppError> {
        if self.trust.public_key.trim().is_empty() {
            return Err(verify_failed(
                "this build of Quirewise has no signing key for components yet",
            ));
        }
        let key = PublicKey::from_base64(self.trust.public_key.trim())
            .map_err(|_| verify_failed("the app's signing key is not valid"))?;
        let signature = Signature::decode(signature)
            .map_err(|_| verify_failed("the signature is unreadable"))?;
        key.verify(data, &signature, false)
            .map_err(|_| verify_failed("the signature does not match"))
    }

    fn fetch_bytes(&self, url: &str, max: u64) -> Result<Vec<u8>, AppError> {
        let mut bytes = Vec::new();
        self.downloader.download(url, max, &mut |chunk| {
            bytes.extend_from_slice(chunk);
            Ok(())
        })?;
        Ok(bytes)
    }

    fn fits(&self, entry: &ComponentEntry) -> bool {
        entry.arch == self.arch || entry.arch == Arch::Any
    }

    /// Moves a verified payload into `components\<id>\<version>\`.
    fn unpack(&self, entry: &ComponentEntry, payload: &Path) -> Result<(), AppError> {
        let target = self.store.version_dir(entry);
        let partial = target.with_extension("partial");
        let _ = fs::remove_dir_all(&partial);
        let result = extract_zip(payload, &partial).and_then(|()| {
            let record =
                serde_json::to_vec_pretty(entry).map_err(|_| AppError::internal("recording"))?;
            fs::write(partial.join(RECORD_FILE), record)
                .map_err(|e| io_error("record the component", &e))?;
            let _ = fs::remove_dir_all(&target);
            fs::rename(&partial, &target).map_err(|e| io_error("install the component", &e))
        });
        if result.is_err() {
            let _ = fs::remove_dir_all(&partial);
            return result;
        }
        // Older versions of the same component are no longer needed.
        if let Ok(versions) = fs::read_dir(self.root.join(&entry.id)) {
            for old in versions.flatten().map(|e| e.path()) {
                if old != target && old.is_dir() {
                    let _ = fs::remove_dir_all(&old);
                }
            }
        }
        Ok(())
    }

    fn scratch_file(&self, name: &str) -> Result<PathBuf, AppError> {
        let dir = self.root.join(DOWNLOAD_DIR);
        fs::create_dir_all(&dir).map_err(|e| io_error("prepare the download folder", &e))?;
        Ok(dir.join(name))
    }
}

/// Unpacks a zip file into `dest` without letting any entry leave it.
fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), AppError> {
    let file = File::open(zip_path).map_err(|e| io_error("read the component", &e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|_| verify_failed("the package is not a zip file"))?;
    if archive.len() > MAX_FILES {
        return Err(verify_failed("the package holds too many files"));
    }
    fs::create_dir_all(dest).map_err(|e| io_error("install the component", &e))?;
    let mut total: u64 = 0;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| verify_failed("the package is damaged"))?;
        let Some(relative) = entry.enclosed_name() else {
            return Err(verify_failed("a file path leaves the install folder"));
        };
        // Links in a package could point outside the install folder.
        if entry
            .unix_mode()
            .is_some_and(|m| m & 0o170_000 == 0o120_000)
        {
            return Err(verify_failed("the package holds a link"));
        }
        let out_path = dest.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|e| io_error("install the component", &e))?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error("install the component", &e))?;
        }
        let mut out = File::create(&out_path).map_err(|e| io_error("install the component", &e))?;
        let remaining = MAX_UNPACKED.saturating_sub(total);
        let written = io::copy(&mut (&mut entry).take(remaining + 1), &mut out)
            .map_err(|e| io_error("install the component", &e))?;
        total += written;
        if total > MAX_UNPACKED {
            return Err(verify_failed(
                "the package unpacks to more than the allowed size",
            ));
        }
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

impl ComponentInstaller for ComponentManager {
    fn available(&self) -> Result<Vec<ComponentEntry>, AppError> {
        let manifest = self.fetch_bytes(&self.trust.manifest_url, MAX_MANIFEST_BYTES as u64)?;
        let signature = self.fetch_bytes(
            &format!("{}.minisig", self.trust.manifest_url),
            MAX_SIGNATURE_BYTES,
        )?;
        let signature = String::from_utf8(signature)
            .map_err(|_| verify_failed("the signature is unreadable"))?;
        self.verify_signature(&manifest, &signature)?;
        let manifest = parse_component_manifest(&manifest)?;
        Ok(manifest
            .components
            .into_iter()
            .filter(|e| self.fits(e))
            .collect())
    }

    fn install(&self, id: &str, progress: &dyn ProgressSink) -> Result<ComponentEntry, AppError> {
        let entry = self
            .available()?
            .into_iter()
            .filter(|e| e.id == id)
            .max_by(|a, b| {
                compare_versions(&a.version, &b.version)
                    .then_with(|| (a.arch == self.arch).cmp(&(b.arch == self.arch)))
            })
            .ok_or_else(|| {
                AppError::new(
                    ErrorCode::ComponentMissing,
                    format!("The component \"{id}\" is not offered for this computer."),
                )
            })?;
        if self.store.installed()?.contains(&entry) {
            return Ok(entry);
        }

        let part = self.scratch_file(&format!("{}-{}.part", entry.id, entry.version))?;
        let result = self
            .download_payload(&entry, &part, progress)
            .and_then(|()| self.unpack(&entry, &part));
        let _ = fs::remove_file(&part);
        result.map(|()| entry)
    }

    fn import(&self, package: &Path) -> Result<ComponentEntry, AppError> {
        let file = File::open(package).map_err(|e| io_error("read the package", &e))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|_| verify_failed("the file is not a Quirewise package"))?;
        let read_small = |archive: &mut zip::ZipArchive<File>, name: &str, max: u64| {
            let entry = archive
                .by_name(name)
                .map_err(|_| verify_failed("the package is incomplete"))?;
            let mut bytes = Vec::new();
            entry
                .take(max + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| io_error("read the package", &e))?;
            if bytes.len() as u64 > max {
                return Err(verify_failed("a package file is too large"));
            }
            Ok(bytes)
        };
        let entry_bytes = read_small(&mut archive, "entry.json", MAX_ENTRY_BYTES)?;
        let signature = read_small(&mut archive, "entry.json.minisig", MAX_SIGNATURE_BYTES)?;
        let signature = String::from_utf8(signature)
            .map_err(|_| verify_failed("the signature is unreadable"))?;
        self.verify_signature(&entry_bytes, &signature)?;
        let entry: ComponentEntry = serde_json::from_slice(&entry_bytes)
            .map_err(|_| verify_failed("the package description is unreadable"))?;
        entry
            .validate()
            .map_err(|e| verify_failed(&e.to_string()))?;
        if !self.fits(&entry) {
            return Err(AppError::new(
                ErrorCode::ComponentMissing,
                "This package is built for a different kind of computer.",
            ));
        }

        let part = self.scratch_file(&format!("{}-{}.import", entry.id, entry.version))?;
        let copied = (|| {
            let mut payload = archive
                .by_name("payload.zip")
                .map_err(|_| verify_failed("the package is incomplete"))?;
            let mut out = File::create(&part).map_err(|e| io_error("unpack the package", &e))?;
            let mut hasher = Sha256::new();
            let mut size: u64 = 0;
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let n = payload
                    .read(&mut buffer)
                    .map_err(|e| io_error("unpack the package", &e))?;
                if n == 0 {
                    break;
                }
                size += n as u64;
                if size > entry.size.get() {
                    return Err(verify_failed("the payload is larger than announced"));
                }
                hasher.update(&buffer[..n]);
                out.write_all(&buffer[..n])
                    .map_err(|e| io_error("unpack the package", &e))?;
            }
            if size != entry.size.get() || hex(&hasher.finalize()) != entry.sha256 {
                return Err(verify_failed("the payload does not match its checksum"));
            }
            Ok(())
        })();
        let result = copied.and_then(|()| self.unpack(&entry, &part));
        let _ = fs::remove_file(&part);
        result.map(|()| entry)
    }

    fn remove(&self, id: &str) -> Result<(), AppError> {
        let valid = !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if !valid || id.starts_with('.') {
            return Err(AppError::new(
                ErrorCode::OptionInvalid,
                "That is not the name of a component.",
            ));
        }
        let dir = self.root.join(id);
        if !dir.exists() {
            return Ok(());
        }
        fs::remove_dir_all(&dir).map_err(|e| io_error("remove the component", &e))
    }
}

impl ComponentManager {
    /// Downloads the payload of `entry` to `part`, checking size and checksum as it arrives.
    fn download_payload(
        &self,
        entry: &ComponentEntry,
        part: &Path,
        progress: &dyn ProgressSink,
    ) -> Result<(), AppError> {
        let total = entry.size.get();
        let mut out = File::create(part).map_err(|e| io_error("save the download", &e))?;
        let mut hasher = Sha256::new();
        let mut done: u64 = 0;
        progress.report(ProgressEvent { done, total });
        self.downloader.download(&entry.url, total, &mut |chunk| {
            hasher.update(chunk);
            out.write_all(chunk)
                .map_err(|e| io_error("save the download", &e))?;
            done += chunk.len() as u64;
            progress.report(ProgressEvent {
                done: done.min(total),
                total,
            });
            Ok(())
        })?;
        out.flush().map_err(|e| io_error("save the download", &e))?;
        if done != total {
            return Err(verify_failed("the download is not the size announced"));
        }
        if hex(&hasher.finalize()) != entry.sha256 {
            return Err(verify_failed("the download does not match its checksum"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::Bytes;
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::sync::Mutex;
    use tempfile::tempdir;

    fn entry(id: &str) -> ComponentEntry {
        ComponentEntry {
            id: id.to_owned(),
            version: "1.0".to_owned(),
            arch: Arch::Any,
            url: "https://example.invalid/x.zip".to_owned(),
            size: Bytes::from_mb(1),
            sha256: "c".repeat(64),
        }
    }

    #[test]
    fn missing_folder_means_nothing_installed() {
        let store = FsComponentStore::new(PathBuf::from(r"Z:\definitely\missing"));
        assert!(store.installed().unwrap().is_empty());
        assert!(store.locate("anything").is_none());
    }

    #[test]
    fn lists_valid_records_and_skips_broken_ones() {
        let dir = tempdir().unwrap();
        let good = dir.path().join("tessdata-eng").join("1.0");
        fs::create_dir_all(&good).unwrap();
        fs::write(
            good.join(RECORD_FILE),
            serde_json::to_vec(&entry("tessdata-eng")).unwrap(),
        )
        .unwrap();
        let broken = dir.path().join("llama").join("2.0");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join(RECORD_FILE), b"{not json").unwrap();
        let incomplete = dir.path().join("verapdf").join("1.0");
        fs::create_dir_all(&incomplete).unwrap();

        let store = FsComponentStore::new(dir.path().to_path_buf());
        let list = store.installed().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "tessdata-eng");
        assert_eq!(store.locate("tessdata-eng"), Some(good));
        assert!(store.locate("llama").is_none());
    }

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert_eq!(compare_versions("24.8.10", "24.8.9"), Ordering::Greater);
        assert_eq!(compare_versions("1.0", "1.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.0", "1.0.1"), Ordering::Less);
        assert_eq!(compare_versions("2.0", "10.0"), Ordering::Less);
    }

    // ---- the installer, with a real signature and a fake network ------------------------------

    struct Network(Mutex<HashMap<String, Vec<u8>>>);

    impl Network {
        fn new() -> Arc<Self> {
            Arc::new(Self(Mutex::new(HashMap::new())))
        }
        fn put(&self, url: &str, bytes: Vec<u8>) {
            self.0.lock().unwrap().insert(url.to_owned(), bytes);
        }
    }

    impl Downloader for Network {
        fn download(
            &self,
            url: &str,
            max_bytes: u64,
            sink: &mut dyn FnMut(&[u8]) -> Result<(), AppError>,
        ) -> Result<(), AppError> {
            let body = self
                .0
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| AppError::new(ErrorCode::NetworkUnavailable, "not found"))?;
            if body.len() as u64 > max_bytes {
                return Err(AppError::new(ErrorCode::NetworkUnavailable, "too large"));
            }
            for chunk in body.chunks(7) {
                sink(chunk)?;
            }
            Ok(())
        }
    }

    struct Keys {
        public: String,
        secret: minisign::SecretKey,
    }

    fn keys() -> Keys {
        let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        Keys {
            public: pair.pk.to_base64(),
            secret: pair.sk,
        }
    }

    fn sign(keys: &Keys, data: &[u8]) -> Vec<u8> {
        let signature = minisign::sign(None, &keys.secret, Cursor::new(data), None, None).unwrap();
        signature.into_string().into_bytes()
    }

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut out));
            let options = zip::write::SimpleFileOptions::default();
            for (name, bytes) in files {
                writer.start_file(*name, options).unwrap();
                writer.write_all(bytes).unwrap();
            }
            writer.finish().unwrap();
        }
        out
    }

    fn entry_for(payload: &[u8], url: &str, version: &str) -> ComponentEntry {
        ComponentEntry {
            id: "libreoffice".to_owned(),
            version: version.to_owned(),
            arch: Arch::Any,
            url: url.to_owned(),
            size: Bytes::new(payload.len() as u64),
            sha256: hex(&Sha256::digest(payload)),
        }
    }

    const MANIFEST_URL: &str = "https://components.invalid/components.json";

    fn publish(net: &Network, keys: &Keys, entries: &[ComponentEntry]) {
        let manifest = serde_json::to_vec(&domain::components::ComponentManifest {
            schema_version: 1,
            components: entries.to_vec(),
        })
        .unwrap();
        net.put(&format!("{MANIFEST_URL}.minisig"), sign(keys, &manifest));
        net.put(MANIFEST_URL, manifest);
    }

    fn manager(dir: &Path, net: &Arc<Network>, keys: &Keys) -> ComponentManager {
        ComponentManager::new(
            dir.to_path_buf(),
            net.clone(),
            ComponentTrust {
                manifest_url: MANIFEST_URL.to_owned(),
                public_key: keys.public.clone(),
            },
            Arch::X64,
        )
    }

    struct Counting(Mutex<Vec<ProgressEvent>>);
    impl ProgressSink for Counting {
        fn report(&self, event: ProgressEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    #[test]
    fn a_signed_component_downloads_verifies_and_installs() {
        let dir = tempdir().unwrap();
        let (net, keys) = (Network::new(), keys());
        let payload = zip_of(&[
            ("program/soffice.exe", b"exe"),
            ("share/readme.txt", b"hello"),
        ]);
        let url = "https://components.invalid/lo-1.zip";
        net.put(url, payload.clone());
        publish(&net, &keys, &[entry_for(&payload, url, "1.0")]);
        let m = manager(dir.path(), &net, &keys);

        let seen = Counting(Mutex::new(Vec::new()));
        let installed = m.install("libreoffice", &seen).unwrap();
        assert_eq!(installed.version, "1.0");
        let folder = dir.path().join("libreoffice").join("1.0");
        assert_eq!(
            fs::read(folder.join("program/soffice.exe")).unwrap(),
            b"exe"
        );
        assert!(folder.join(RECORD_FILE).is_file());
        assert_eq!(m.store.locate("libreoffice"), Some(folder));
        let events = seen.0.lock().unwrap();
        assert_eq!(events.last().map(|e| e.done), Some(payload.len() as u64));
        assert!(
            fs::read_dir(dir.path().join(DOWNLOAD_DIR))
                .unwrap()
                .next()
                .is_none(),
            "the download is cleaned up"
        );

        // A second request changes nothing.
        assert_eq!(m.install("libreoffice", &seen).unwrap(), installed);
    }

    #[test]
    fn a_newer_version_replaces_the_older_one() {
        let dir = tempdir().unwrap();
        let (net, keys) = (Network::new(), keys());
        let v1 = zip_of(&[("a.txt", b"one")]);
        let v2 = zip_of(&[("a.txt", b"two")]);
        net.put("https://components.invalid/1.zip", v1.clone());
        net.put("https://components.invalid/2.zip", v2.clone());
        publish(
            &net,
            &keys,
            &[entry_for(&v1, "https://components.invalid/1.zip", "1.0")],
        );
        let m = manager(dir.path(), &net, &keys);
        m.install("libreoffice", &application::NoProgress).unwrap();
        publish(
            &net,
            &keys,
            &[
                entry_for(&v1, "https://components.invalid/1.zip", "1.0"),
                entry_for(&v2, "https://components.invalid/2.zip", "1.10"),
            ],
        );
        m.install("libreoffice", &application::NoProgress).unwrap();
        assert!(!dir.path().join("libreoffice").join("1.0").exists());
        assert_eq!(
            fs::read(dir.path().join("libreoffice").join("1.10").join("a.txt")).unwrap(),
            b"two"
        );
    }

    #[test]
    fn a_build_without_a_signing_key_installs_nothing() {
        let dir = tempdir().unwrap();
        let (net, keys) = (Network::new(), keys());
        let payload = zip_of(&[("a.txt", b"x")]);
        net.put("https://components.invalid/p.zip", payload.clone());
        publish(
            &net,
            &keys,
            &[entry_for(
                &payload,
                "https://components.invalid/p.zip",
                "1.0",
            )],
        );
        let m = ComponentManager::new(
            dir.path().to_path_buf(),
            net.clone(),
            ComponentTrust {
                manifest_url: MANIFEST_URL.to_owned(),
                public_key: String::new(),
            },
            Arch::X64,
        );
        let err = m
            .install("libreoffice", &application::NoProgress)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentVerifyFailed);
        assert!(err.message.contains("no signing key"), "{}", err.message);
        assert!(default_trust().public_key.is_empty() || default_trust().public_key.len() > 20);
    }

    #[test]
    fn a_list_signed_by_someone_else_is_refused() {
        let dir = tempdir().unwrap();
        let (net, trusted, stranger) = (Network::new(), keys(), keys());
        let payload = zip_of(&[("a.txt", b"x")]);
        net.put("https://components.invalid/p.zip", payload.clone());
        publish(
            &net,
            &stranger,
            &[entry_for(
                &payload,
                "https://components.invalid/p.zip",
                "1.0",
            )],
        );
        let err = manager(dir.path(), &net, &trusted)
            .install("libreoffice", &application::NoProgress)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentVerifyFailed);
        assert!(!dir.path().join("libreoffice").exists());
    }

    #[test]
    fn a_tampered_list_or_payload_is_refused() {
        let dir = tempdir().unwrap();
        let (net, keys) = (Network::new(), keys());
        let payload = zip_of(&[("a.txt", b"x")]);
        let url = "https://components.invalid/p.zip";
        publish(&net, &keys, &[entry_for(&payload, url, "1.0")]);
        // The payload on the server is not the one that was signed for.
        let mut other = payload.clone();
        let last = other.len() - 1;
        other[last] ^= 0xFF;
        net.put(url, other);
        let m = manager(dir.path(), &net, &keys);
        let err = m
            .install("libreoffice", &application::NoProgress)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentVerifyFailed);
        assert!(!dir.path().join("libreoffice").exists());

        // The list changed after it was signed.
        let signed = net.0.lock().unwrap().get(MANIFEST_URL).cloned().unwrap();
        let altered = String::from_utf8(signed).unwrap().replace("1.0", "9.9");
        net.put(MANIFEST_URL, altered.into_bytes());
        let err = m.available().unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentVerifyFailed);
    }

    #[test]
    fn components_for_other_architectures_are_not_offered() {
        let dir = tempdir().unwrap();
        let (net, keys) = (Network::new(), keys());
        let payload = zip_of(&[("a.txt", b"x")]);
        let mut arm = entry_for(&payload, "https://components.invalid/p.zip", "1.0");
        arm.arch = Arch::Arm64;
        publish(&net, &keys, &[arm]);
        let m = manager(dir.path(), &net, &keys);
        assert!(m.available().unwrap().is_empty());
        let err = m
            .install("libreoffice", &application::NoProgress)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ComponentMissing);
    }

    #[test]
    fn an_unavailable_network_is_reported_as_such() {
        let dir = tempdir().unwrap();
        let (net, keys) = (Network::new(), keys());
        let err = manager(dir.path(), &net, &keys).available().unwrap_err();
        assert_eq!(err.code, ErrorCode::NetworkUnavailable);
    }

    fn package(keys: &Keys, entry: &ComponentEntry, payload: &[u8]) -> Vec<u8> {
        let entry_json = serde_json::to_vec(entry).unwrap();
        let signature = sign(keys, &entry_json);
        zip_of(&[
            ("entry.json", &entry_json),
            ("entry.json.minisig", &signature),
            ("payload.zip", payload),
        ])
    }

    #[test]
    fn an_offline_package_installs_after_the_same_checks() {
        let dir = tempdir().unwrap();
        let (net, keys) = (Network::new(), keys());
        let payload = zip_of(&[("tessdata/eng.traineddata", b"data")]);
        let mut e = entry_for(&payload, "https://components.invalid/x.zip", "1.0");
        e.id = "tessdata-eng".to_owned();
        let file = dir.path().join("eng.qwpkg");
        fs::write(&file, package(&keys, &e, &payload)).unwrap();
        let m = manager(&dir.path().join("components"), &net, &keys);
        let installed = m.import(&file).unwrap();
        assert_eq!(installed.id, "tessdata-eng");
        assert_eq!(
            fs::read(
                dir.path()
                    .join("components/tessdata-eng/1.0/tessdata/eng.traineddata")
            )
            .unwrap(),
            b"data"
        );
    }

    #[test]
    fn an_offline_package_with_a_bad_signature_or_payload_is_refused() {
        let dir = tempdir().unwrap();
        let (net, keys, stranger) = (Network::new(), keys(), keys());
        let payload = zip_of(&[("a.txt", b"x")]);
        let e = entry_for(&payload, "https://components.invalid/x.zip", "1.0");
        let m = manager(&dir.path().join("components"), &net, &keys);

        let forged = dir.path().join("forged.qwpkg");
        fs::write(&forged, package(&stranger, &e, &payload)).unwrap();
        assert_eq!(
            m.import(&forged).unwrap_err().code,
            ErrorCode::ComponentVerifyFailed
        );

        let swapped = dir.path().join("swapped.qwpkg");
        let other = zip_of(&[("a.txt", b"different")]);
        fs::write(&swapped, package(&keys, &e, &other)).unwrap();
        assert_eq!(
            m.import(&swapped).unwrap_err().code,
            ErrorCode::ComponentVerifyFailed
        );

        let junk = dir.path().join("junk.qwpkg");
        fs::write(&junk, b"not a zip").unwrap();
        assert_eq!(
            m.import(&junk).unwrap_err().code,
            ErrorCode::ComponentVerifyFailed
        );
        assert!(!dir.path().join("components/libreoffice").exists());
    }

    #[test]
    fn a_package_cannot_write_outside_its_folder() {
        let dir = tempdir().unwrap();
        let (net, keys) = (Network::new(), keys());
        let payload = zip_of(&[("../escape.txt", b"bad")]);
        let e = entry_for(&payload, "https://components.invalid/x.zip", "1.0");
        let file = dir.path().join("evil.qwpkg");
        fs::write(&file, package(&keys, &e, &payload)).unwrap();
        let root = dir.path().join("components");
        let m = manager(&root, &net, &keys);
        assert_eq!(
            m.import(&file).unwrap_err().code,
            ErrorCode::ComponentVerifyFailed
        );
        assert!(!dir.path().join("escape.txt").exists());
        assert!(!root.join("escape.txt").exists());
        assert!(!root.join("libreoffice").join("1.0").exists());
    }

    #[test]
    fn removing_deletes_every_version_and_refuses_odd_names() {
        let dir = tempdir().unwrap();
        let (net, keys) = (Network::new(), keys());
        let payload = zip_of(&[("a.txt", b"x")]);
        let url = "https://components.invalid/p.zip";
        net.put(url, payload.clone());
        publish(&net, &keys, &[entry_for(&payload, url, "1.0")]);
        let m = manager(dir.path(), &net, &keys);
        m.install("libreoffice", &application::NoProgress).unwrap();
        m.remove("libreoffice").unwrap();
        assert!(m.store.installed().unwrap().is_empty());
        m.remove("libreoffice").unwrap();
        for bad in ["", "..", "../x", ".download", "A"] {
            assert_eq!(
                m.remove(bad).unwrap_err().code,
                ErrorCode::OptionInvalid,
                "{bad}"
            );
        }
    }
}
