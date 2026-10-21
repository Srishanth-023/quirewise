//! The Explorer right-click menu and the "Open with" entry, written to the current user's part of
//! the registry with `reg.exe` (no administrator rights, no extra libraries, no unsafe code).
//!
//! The classic menu is shown by Windows 10 and, under "Show more options", by Windows 11. A
//! top-level Windows 11 entry needs a packaged shell extension and is left out (see ADR-0012).
//!
//! Layout under `Software\Classes`:
//!
//! - `SystemFileAssociations\.pdf\shell\Quirewise` is the submenu; its `ExtendedSubCommandsKey`
//!   points at `Quirewise.PdfMenu`, whose `shell` keys are the entries.
//! - `Applications\quirewise.exe` lists Quirewise in "Open with" for `.pdf`.

use application::AppError;
use domain::ErrorCode;
use domain::explorer::{MenuItem, validate_menu};
use std::path::Path;
use std::process::Command;

/// The registry root the keys go under, for the real thing.
pub const CLASSES_ROOT: &str = r"HKCU\Software\Classes";

const SUBMENU: &str = r"SystemFileAssociations\.pdf\shell\Quirewise";
const MENU_KEY: &str = "Quirewise.PdfMenu";
const APPLICATION: &str = r"Applications\quirewise.exe";

/// One value to write. `name` of `None` is the default value of the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegValue {
    /// Key below the classes root.
    pub key: String,
    /// Value name, or `None` for the default value.
    pub name: Option<String>,
    /// Text of the value.
    pub data: String,
}

fn value(key: impl Into<String>, name: Option<&str>, data: impl Into<String>) -> RegValue {
    RegValue {
        key: key.into(),
        name: name.map(str::to_owned),
        data: data.into(),
    }
}

/// The command line of an entry. `%1` is the file Explorer passes; the quotes keep paths with
/// spaces in one piece and `--` makes sure a file name is never read as an option.
fn command_line(exe: &Path, tool: Option<&str>) -> String {
    let exe = exe.display();
    match tool {
        Some(tool) => format!("\"{exe}\" --tool {tool} -- \"%1\""),
        None => format!("\"{exe}\" -- \"%1\""),
    }
}

/// Every value the menu and the "Open with" entry consist of. Pure, so it can be tested without
/// touching the registry.
pub fn plan(exe: &Path, items: &[MenuItem], open_title: &str) -> Vec<RegValue> {
    let icon = format!("\"{}\",0", exe.display());
    let mut out = vec![
        value(SUBMENU, Some("MUIVerb"), "Quirewise"),
        value(SUBMENU, Some("Icon"), icon.clone()),
        value(SUBMENU, Some("ExtendedSubCommandsKey"), MENU_KEY),
        value(
            format!(r"{MENU_KEY}\shell\00open"),
            Some("MUIVerb"),
            open_title,
        ),
        value(
            format!(r"{MENU_KEY}\shell\00open\command"),
            None,
            command_line(exe, None),
        ),
    ];
    for (i, item) in items.iter().enumerate() {
        let verb = format!(r"{MENU_KEY}\shell\{:02}{}", i + 1, item.tool);
        out.push(value(verb.clone(), Some("MUIVerb"), item.title.trim()));
        out.push(value(
            format!(r"{verb}\command"),
            None,
            command_line(exe, Some(item.tool.as_str())),
        ));
    }
    out.push(value(APPLICATION, Some("FriendlyAppName"), "Quirewise"));
    out.push(value(
        format!(r"{APPLICATION}\shell\open\command"),
        None,
        command_line(exe, None),
    ));
    out.push(value(
        format!(r"{APPLICATION}\SupportedTypes"),
        Some(".pdf"),
        "",
    ));
    out
}

/// The keys to remove to take everything away again.
pub const fn keys_to_remove() -> [&'static str; 3] {
    [SUBMENU, MENU_KEY, APPLICATION]
}

/// What the installer and the settings screen need from the registry.
pub trait Registry {
    /// Writes one value, creating the key.
    fn set(&self, value: &RegValue) -> Result<(), AppError>;
    /// Deletes a key and everything below it. A key that is not there is fine.
    fn delete_tree(&self, key: &str) -> Result<(), AppError>;
    /// True when the key exists.
    fn exists(&self, key: &str) -> bool;
}

/// Writes the menu. Anything it wrote before is removed first, so a smaller menu does not leave
/// old entries behind. A failure part-way removes what was written.
pub fn install(
    registry: &dyn Registry,
    exe: &Path,
    items: &[MenuItem],
    open_title: &str,
) -> Result<(), AppError> {
    validate_menu(items)?;
    if open_title.trim().is_empty() {
        return Err(
            domain::DomainError::option("explorer menu", "the text of an entry is empty").into(),
        );
    }
    uninstall(registry)?;
    for value in plan(exe, items, open_title) {
        if let Err(e) = registry.set(&value) {
            let _ = uninstall(registry);
            return Err(e);
        }
    }
    Ok(())
}

/// Removes the menu and the "Open with" entry.
pub fn uninstall(registry: &dyn Registry) -> Result<(), AppError> {
    for key in keys_to_remove() {
        registry.delete_tree(key)?;
    }
    Ok(())
}

/// True when the submenu is in the registry.
pub fn is_installed(registry: &dyn Registry) -> bool {
    registry.exists(SUBMENU)
}

/// [`Registry`] over `reg.exe`, below `root` (normally [`CLASSES_ROOT`]).
#[derive(Debug, Clone)]
pub struct RegExe {
    root: String,
}

impl RegExe {
    /// The current user's classes.
    pub fn current_user() -> Self {
        Self::below(CLASSES_ROOT)
    }

    /// Any root, for tests that must not touch the real keys.
    pub fn below(root: &str) -> Self {
        Self {
            root: root.trim_end_matches('\\').to_owned(),
        }
    }

    fn path(&self, key: &str) -> String {
        format!("{}\\{key}", self.root)
    }

    fn reg(args: &[&str]) -> std::io::Result<std::process::Output> {
        let mut command = Command::new("reg.exe");
        command.args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // No console window flashes up when the app (a windows program) runs reg.exe.
            command.creation_flags(0x0800_0000);
        }
        command.output()
    }
}

fn failed(what: &str) -> AppError {
    AppError::new(
        ErrorCode::Internal,
        format!(
            "Windows did not accept the change to the Explorer menu ({what}). Nothing else was changed."
        ),
    )
}

impl Registry for RegExe {
    fn set(&self, v: &RegValue) -> Result<(), AppError> {
        let path = self.path(&v.key);
        let mut args = vec!["add", path.as_str()];
        match &v.name {
            Some(name) => args.extend(["/v", name.as_str()]),
            None => args.push("/ve"),
        }
        args.extend(["/t", "REG_SZ", "/d", v.data.as_str(), "/f"]);
        match Self::reg(&args) {
            Ok(out) if out.status.success() => Ok(()),
            Ok(_) => Err(failed("write")),
            Err(e) => Err(AppError::from_io("change the Explorer menu", &e)),
        }
    }

    fn delete_tree(&self, key: &str) -> Result<(), AppError> {
        let path = self.path(key);
        if !self.exists(key) {
            return Ok(());
        }
        match Self::reg(&["delete", path.as_str(), "/f"]) {
            Ok(out) if out.status.success() => Ok(()),
            Ok(_) => Err(failed("remove")),
            Err(e) => Err(AppError::from_io("change the Explorer menu", &e)),
        }
    }

    fn exists(&self, key: &str) -> bool {
        let path = self.path(key);
        Self::reg(&["query", path.as_str()]).is_ok_and(|o| o.status.success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::ToolId;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn items() -> Vec<MenuItem> {
        vec![
            MenuItem {
                tool: ToolId::Compress,
                title: "Compress".into(),
            },
            MenuItem {
                tool: ToolId::SplitRanges,
                title: "Split by ranges".into(),
            },
        ]
    }

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\Quirewise\quirewise.exe")
    }

    #[test]
    fn the_plan_has_the_submenu_the_entries_and_open_with() {
        let plan = plan(&exe(), &items(), "Open with Quirewise");
        let get = |key: &str, name: Option<&str>| {
            plan.iter()
                .find(|v| v.key == key && v.name.as_deref() == name)
                .map(|v| v.data.clone())
        };
        assert_eq!(
            get(SUBMENU, Some("ExtendedSubCommandsKey")).as_deref(),
            Some("Quirewise.PdfMenu")
        );
        assert_eq!(
            get(r"Quirewise.PdfMenu\shell\00open\command", None).as_deref(),
            Some(r#""C:\Program Files\Quirewise\quirewise.exe" -- "%1""#)
        );
        assert_eq!(
            get(r"Quirewise.PdfMenu\shell\01compress", Some("MUIVerb")).as_deref(),
            Some("Compress")
        );
        assert_eq!(
            get(r"Quirewise.PdfMenu\shell\02split-ranges\command", None).as_deref(),
            Some(r#""C:\Program Files\Quirewise\quirewise.exe" --tool split-ranges -- "%1""#)
        );
        assert_eq!(
            get(r"Applications\quirewise.exe\SupportedTypes", Some(".pdf")).as_deref(),
            Some("")
        );
        assert!(get(r"Applications\quirewise.exe\shell\open\command", None).is_some());
    }

    /// A registry in memory that records what was written.
    #[derive(Default)]
    struct Memory {
        values: RefCell<BTreeMap<(String, Option<String>), String>>,
        fail_on: RefCell<Option<String>>,
    }

    impl Registry for Memory {
        fn set(&self, v: &RegValue) -> Result<(), AppError> {
            if self
                .fail_on
                .borrow()
                .as_deref()
                .is_some_and(|k| v.key.contains(k))
            {
                return Err(failed("write"));
            }
            self.values
                .borrow_mut()
                .insert((v.key.clone(), v.name.clone()), v.data.clone());
            Ok(())
        }
        fn delete_tree(&self, key: &str) -> Result<(), AppError> {
            self.values
                .borrow_mut()
                .retain(|(k, _), _| k != key && !k.starts_with(&format!("{key}\\")));
            Ok(())
        }
        fn exists(&self, key: &str) -> bool {
            self.values
                .borrow()
                .keys()
                .any(|(k, _)| k == key || k.starts_with(&format!("{key}\\")))
        }
    }

    #[test]
    fn install_replaces_the_old_menu_and_uninstall_removes_everything() {
        let reg = Memory::default();
        assert!(!is_installed(&reg));
        install(&reg, &exe(), &items(), "Open").unwrap();
        assert!(is_installed(&reg));
        let before = reg.values.borrow().len();

        install(&reg, &exe(), &items()[..1], "Open").unwrap();
        assert!(
            reg.values.borrow().len() < before,
            "the smaller menu left nothing behind"
        );
        assert!(!reg.exists(r"Quirewise.PdfMenu\shell\02split-ranges"));

        uninstall(&reg).unwrap();
        assert!(reg.values.borrow().is_empty());
        assert!(!is_installed(&reg));
    }

    #[test]
    fn a_bad_menu_is_refused_before_anything_is_written() {
        let reg = Memory::default();
        let bad = vec![MenuItem {
            tool: ToolId::EditPdf,
            title: "Edit".into(),
        }];
        assert!(install(&reg, &exe(), &bad, "Open").is_err());
        assert!(install(&reg, &exe(), &items(), "  ").is_err());
        assert!(reg.values.borrow().is_empty());
    }

    #[test]
    fn a_failure_part_way_leaves_nothing_behind() {
        let reg = Memory::default();
        *reg.fail_on.borrow_mut() = Some("02split-ranges".into());
        assert!(install(&reg, &exe(), &items(), "Open").is_err());
        assert!(reg.values.borrow().is_empty());
    }

    /// The real `reg.exe`, below a key of its own that is removed afterwards.
    #[cfg(windows)]
    #[test]
    fn reg_exe_writes_reads_and_removes_the_menu() {
        let root = format!(r"HKCU\Software\QuirewiseTest-{}", std::process::id());
        let reg = RegExe::below(&format!(r"{root}\Classes"));
        let guard = Cleanup(root.clone());

        install(&reg, &exe(), &items(), "Open with Quirewise").unwrap();
        assert!(is_installed(&reg));
        let out = RegExe::reg(&[
            "query",
            &format!(r"{root}\Classes\Quirewise.PdfMenu\shell\01compress\command"),
            "/ve",
        ])
        .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        assert!(
            text.contains(r#""C:\Program Files\Quirewise\quirewise.exe" --tool compress -- "%1""#),
            "{text}"
        );
        let menu = RegExe::reg(&[
            "query",
            &format!(r"{root}\Classes\SystemFileAssociations\.pdf\shell\Quirewise"),
        ])
        .unwrap();
        let menu = String::from_utf8_lossy(&menu.stdout).into_owned();
        assert!(
            menu.contains("ExtendedSubCommandsKey") && menu.contains("Quirewise.PdfMenu"),
            "{menu}"
        );

        uninstall(&reg).unwrap();
        assert!(!is_installed(&reg));
        drop(guard);
    }

    #[cfg(windows)]
    struct Cleanup(String);

    #[cfg(windows)]
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = RegExe::reg(&["delete", &self.0, "/f"]);
        }
    }
}
