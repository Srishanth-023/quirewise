//! The Quirewise submenu of the Explorer right-click menu: which tools it offers and how the
//! launch arguments of a menu entry look. Writing the registry is infrastructure's job.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::tools::ToolId;

/// Most tools the submenu offers besides "Open with Quirewise".
pub const MAX_MENU_TOOLS: usize = 5;
/// Longest menu text, in characters.
pub const MAX_TITLE_CHARS: usize = 60;
/// Most files one launch may carry. Explorer starts the entry once per file (up to 15 at a time),
/// and a drop or a script can add more.
pub const MAX_LAUNCH_FILES: usize = 500;

/// The tools offered until the person picks others.
pub const DEFAULT_MENU_TOOLS: [ToolId; MAX_MENU_TOOLS] = [
    ToolId::Compress,
    ToolId::Merge,
    ToolId::SplitRanges,
    ToolId::Rotate,
    ToolId::Ocr,
];

/// Tools that make sense from a right-click on a PDF: they read PDFs and need no editor.
pub const fn offered_in_menu(tool: ToolId) -> bool {
    use crate::recipe::ChainRole;
    !matches!(ChainRole::of(tool), ChainRole::Excluded | ChainRole::Source)
        || matches!(
            tool,
            ToolId::Merge | ToolId::AlternateMix | ToolId::Organize
        )
}

/// One tool in the submenu, with the text shown for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MenuItem {
    /// The tool the entry starts.
    pub tool: ToolId,
    /// Menu text in the language of the app.
    pub title: String,
}

/// Checks the items for a submenu: a few tools that fit a right-click, each once, with plain text.
pub fn validate_menu(items: &[MenuItem]) -> Result<(), DomainError> {
    if items.len() > MAX_MENU_TOOLS {
        return Err(DomainError::option(
            "explorer menu",
            format!("offer at most {MAX_MENU_TOOLS} tools"),
        ));
    }
    for (i, item) in items.iter().enumerate() {
        if !offered_in_menu(item.tool) {
            return Err(DomainError::option(
                "explorer menu",
                format!("{} cannot start from a right-click", item.tool),
            ));
        }
        if items[..i].iter().any(|other| other.tool == item.tool) {
            return Err(DomainError::option(
                "explorer menu",
                format!("{} is listed twice", item.tool),
            ));
        }
        let title = item.title.trim();
        if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
            return Err(DomainError::option(
                "explorer menu",
                format!("the text of an entry must have 1 to {MAX_TITLE_CHARS} characters"),
            ));
        }
        // The text goes into a registry value that Explorer shows as it is.
        if title.chars().any(char::is_control) || title.contains('&') || title.contains('"') {
            return Err(DomainError::option(
                "explorer menu",
                "the text of an entry cannot contain quotes, ampersands or control characters",
            ));
        }
    }
    Ok(())
}

/// What an Explorer entry asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Launch {
    /// The tool to select, when the entry names one.
    pub tool: Option<ToolId>,
    /// The files to add to the work list.
    pub files: Vec<std::path::PathBuf>,
}

/// Reads the command line of the app (without the program name): `[--tool <id>] [--] files...`.
///
/// Unknown options are errors rather than files, so a typo never becomes a path. More than
/// [`MAX_LAUNCH_FILES`] files are cut off.
pub fn parse_launch<I, S>(args: I) -> Result<Launch, DomainError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut launch = Launch::default();
    let mut only_files = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let arg = arg.as_ref();
        if only_files || !arg.starts_with("--") {
            if launch.files.len() < MAX_LAUNCH_FILES {
                launch.files.push(arg.into());
            }
        } else if arg == "--" {
            only_files = true;
        } else if arg == "--tool" {
            let id = args
                .next()
                .ok_or_else(|| DomainError::option("--tool", "name a tool"))?;
            launch.tool = Some(id.as_ref().parse()?);
        } else {
            return Err(DomainError::option(
                "command line",
                format!("unknown option {arg}"),
            ));
        }
    }
    Ok(launch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(tool: ToolId, title: &str) -> MenuItem {
        MenuItem {
            tool,
            title: title.into(),
        }
    }

    #[test]
    fn the_default_tools_all_fit_a_right_click() {
        for tool in DEFAULT_MENU_TOOLS {
            assert!(offered_in_menu(tool), "{tool}");
        }
        assert!(offered_in_menu(ToolId::Merge));
        assert!(!offered_in_menu(ToolId::EditPdf));
        assert!(!offered_in_menu(ToolId::ScanToPdf));
        assert!(!offered_in_menu(ToolId::WordToPdf));
    }

    #[test]
    fn a_menu_is_checked() {
        validate_menu(&[
            item(ToolId::Compress, "Compress"),
            item(ToolId::Merge, "Merge"),
        ])
        .unwrap();
        validate_menu(&[]).unwrap();
        let six: Vec<MenuItem> = [
            ToolId::Compress,
            ToolId::Merge,
            ToolId::Rotate,
            ToolId::Ocr,
            ToolId::Flatten,
            ToolId::Sanitize,
        ]
        .into_iter()
        .map(|t| item(t, "x"))
        .collect();
        assert!(validate_menu(&six).is_err());
        assert!(validate_menu(&[item(ToolId::EditPdf, "Edit")]).is_err());
        assert!(validate_menu(&[item(ToolId::Rotate, "A"), item(ToolId::Rotate, "B")]).is_err());
        for bad in [
            "",
            "   ",
            "a\"b",
            "R&D",
            "tab\there",
            &"x".repeat(MAX_TITLE_CHARS + 1),
        ] {
            assert!(
                validate_menu(&[item(ToolId::Rotate, bad)]).is_err(),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn launch_arguments_are_read() {
        let l = parse_launch(["--tool", "compress", "--", "C:\\a b.pdf"]).unwrap();
        assert_eq!(l.tool, Some(ToolId::Compress));
        assert_eq!(l.files, vec![std::path::PathBuf::from("C:\\a b.pdf")]);

        let plain = parse_launch(["C:\\a.pdf", "C:\\b.pdf"]).unwrap();
        assert_eq!(plain.tool, None);
        assert_eq!(plain.files.len(), 2);

        assert_eq!(
            parse_launch(Vec::<String>::new()).unwrap(),
            Launch::default()
        );
    }

    #[test]
    fn a_file_named_like_an_option_needs_the_separator() {
        let l = parse_launch(["--", "--tool"]).unwrap();
        assert_eq!(l.files, vec![std::path::PathBuf::from("--tool")]);
        assert!(parse_launch(["--tool"]).is_err());
        assert!(parse_launch(["--tool", "nope"]).is_err());
        assert!(parse_launch(["--bogus", "a.pdf"]).is_err());
    }

    #[test]
    fn too_many_files_are_cut_off() {
        let many: Vec<String> = (0..MAX_LAUNCH_FILES + 20)
            .map(|i| format!("f{i}.pdf"))
            .collect();
        assert_eq!(parse_launch(&many).unwrap().files.len(), MAX_LAUNCH_FILES);
    }
}
