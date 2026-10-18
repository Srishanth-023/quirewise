//! `qw`: headless access to Quirewise rules and the tool registry, for tests and power users.
//!
//! Tool subcommands are added per phase through the registry; nothing here hardcodes tools.
#![forbid(unsafe_code)]

use application::ports::{Clock, FileStore};
use application::query::{DocumentQuery, QueryResult, QueryRunner};
use application::{AppError, CancellationToken, Password, ToolRegistry};
use clap::{Parser, Subcommand};
use domain::naming::{NameContext, OutputNamingPattern};
use domain::path_rules::{UncPolicy, check_path_text};
use domain::{Duplicates, PageRange, ToolId};
use infrastructure::{AtomicFileStore, SystemClock, WorkerSession};
use std::path::PathBuf;

/// Command line of `qw`.
#[derive(Debug, Parser)]
#[command(name = "qw", version, about = "Quirewise command line")]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print the version.
    Version,
    /// Inspect the tool registry.
    Tools {
        /// Registry action.
        #[command(subcommand)]
        action: ToolsAction,
    },
    /// Check a page range expression against a page count.
    Ranges {
        /// Expression such as `1,3,5-8,last`.
        expression: String,
        /// Page count of the document.
        #[arg(long)]
        pages: u32,
        /// Allow the same page twice (merge, organize, alternate and mix).
        #[arg(long)]
        allow_duplicates: bool,
    },
    /// Preview an output file name from a naming pattern.
    Name {
        /// Pattern, for example `{filename} ({tool})`.
        #[arg(long, default_value = domain::naming::DEFAULT_PATTERN)]
        pattern: String,
        /// Source name without extension.
        #[arg(long)]
        filename: String,
        /// Tool label.
        #[arg(long, default_value = "tool")]
        tool: String,
        /// Sequence number for `{n}`.
        #[arg(long)]
        n: Option<u32>,
        /// Page for `{page}`.
        #[arg(long)]
        page: Option<u32>,
        /// Page count for `{pages}`.
        #[arg(long)]
        pages: Option<u32>,
        /// Text for `{text}`.
        #[arg(long)]
        text: Option<String>,
        /// Bookmark for `{bookmark}`.
        #[arg(long)]
        bookmark: Option<String>,
    },
    /// Print the JSON Schema of the types the UI exchanges with the core.
    Schema,
    /// Inspect a PDF through the worker and print the report as JSON.
    Inspect {
        /// PDF to inspect.
        path: PathBuf,
        /// Read the password from the first line of standard input.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Render one page through the worker and write it as a PNG.
    Thumbnail {
        /// PDF to render.
        path: PathBuf,
        /// Zero-based page index.
        #[arg(long, default_value_t = 0)]
        page: u32,
        /// Longest edge in pixels.
        #[arg(long, default_value_t = 256)]
        edge: u32,
        /// Output PNG path. An existing file is never overwritten.
        #[arg(long)]
        out: PathBuf,
        /// Read the password from the first line of standard input.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Check a path against the path policy.
    Path {
        /// Path to check.
        path: PathBuf,
        /// Treat as user-picked, which allows UNC paths.
        #[arg(long)]
        user_picked: bool,
    },
}

/// Registry actions.
#[derive(Debug, Subcommand)]
pub enum ToolsAction {
    /// List registered tools as JSON.
    List,
    /// Print the options JSON Schema of one tool.
    Schema {
        /// Tool id, for example `rotate`.
        id: String,
    },
}

fn json(value: &impl serde::Serialize) -> Result<String, AppError> {
    serde_json::to_string_pretty(value).map_err(|_| AppError::internal("formatting the output"))
}

/// Everything a command may use, injected so tests can fake the worker.
pub struct Env<'a> {
    /// Tool registry.
    pub registry: &'a ToolRegistry,
    /// Clock for naming previews.
    pub clock: &'a dyn Clock,
    /// Document queries (inspect, thumbnails).
    pub queries: &'a dyn QueryRunner,
    /// File store for outputs.
    pub files: &'a dyn FileStore,
    /// Password read from standard input, when requested.
    pub password: Option<Password>,
}

/// Runs a command and returns the text to print.
pub fn execute(cli: Cli, env: Env<'_>) -> Result<String, AppError> {
    let Env {
        registry,
        clock,
        queries,
        files,
        password,
    } = env;
    match cli.command {
        Command::Version => Ok(format!("qw {}", env!("CARGO_PKG_VERSION"))),
        Command::Tools { action } => match action {
            ToolsAction::List => json(&registry.list()),
            ToolsAction::Schema { id } => {
                let id: ToolId = id.parse()?;
                let tool = registry.get(id).ok_or_else(|| {
                    AppError::new(
                        domain::ErrorCode::ComponentMissing,
                        format!("The {id} tool is not registered in this build."),
                    )
                })?;
                json(&tool.options_schema())
            }
        },
        Command::Ranges {
            expression,
            pages,
            allow_duplicates,
        } => {
            let policy = if allow_duplicates {
                Duplicates::Allow
            } else {
                Duplicates::Reject
            };
            let selection = PageRange::parse(&expression)?.resolve(pages, policy)?;
            let list: Vec<String> = selection
                .pages()
                .iter()
                .map(|p| p.to_number().get().to_string())
                .collect();
            Ok(list.join(","))
        }
        Command::Name {
            pattern,
            filename,
            tool,
            n,
            page,
            pages,
            text,
            bookmark,
        } => {
            let pattern = OutputNamingPattern::parse(&pattern)?;
            let ctx = NameContext {
                filename: &filename,
                tool: &tool,
                now: clock.now(),
                sequence: n,
                page,
                pages,
                bookmark: bookmark.as_deref(),
                text: text.as_deref(),
            };
            Ok(pattern.render(&ctx)?)
        }
        Command::Schema => Ok(application::dto::ipc_schema_json()),
        Command::Inspect { path, .. } => {
            let query = DocumentQuery::Inspect { path, password };
            match queries.query(&query, &CancellationToken::new())? {
                QueryResult::Inspect(report) => json(&*report),
                QueryResult::Thumbnail(_) => Err(AppError::internal("inspecting the file")),
            }
        }
        Command::Thumbnail {
            path,
            page,
            edge,
            out,
            ..
        } => {
            let query = DocumentQuery::Thumbnail {
                path,
                password,
                page,
                max_edge: edge,
            };
            let QueryResult::Thumbnail(thumb) = queries.query(&query, &CancellationToken::new())?
            else {
                return Err(AppError::internal("rendering the page"));
            };
            let written =
                files.write_atomic(&out, &mut |w| w.write_all(&thumb.png.0), &|_| Ok(()))?;
            Ok(format!(
                "{} ({}x{}, page {} of {})",
                written.display(),
                thumb.width,
                thumb.height,
                page + 1,
                thumb.page_count
            ))
        }
        Command::Path { path, user_picked } => {
            let policy = if user_picked {
                UncPolicy::UserPicked
            } else {
                UncPolicy::Deny
            };
            check_path_text(&path.to_string_lossy(), policy)?;
            Ok("allowed".to_owned())
        }
    }
}

/// Builds the registry the CLI uses: the same one the worker uses.
pub fn default_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    application::tools::register_all(&mut registry);
    registry
}

/// The worker executable next to the running one.
fn worker_exe() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("quirewise-worker.exe")))
        .unwrap_or_else(|| PathBuf::from("quirewise-worker.exe"))
}

fn read_password_line() -> Option<Password> {
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok()?;
    let trimmed = line.trim_end_matches(&['\r', '\n'][..]);
    (!trimmed.is_empty()).then(|| Password::new(trimmed))
}

/// Entry point shared by `main` and tests. Returns the text and the process exit code.
pub fn run<I, T>(args: I) -> (String, i32)
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(e) => return (e.to_string(), if e.use_stderr() { 2 } else { 0 }),
    };
    let password = match &cli.command {
        Command::Inspect {
            password_stdin: true,
            ..
        }
        | Command::Thumbnail {
            password_stdin: true,
            ..
        } => read_password_line(),
        _ => None,
    };
    let session = WorkerSession::new(worker_exe(), domain::limits::SafetyLimits::default());
    let env = Env {
        registry: &default_registry(),
        clock: &SystemClock,
        queries: &session,
        files: &AtomicFileStore,
        password,
    };
    match execute(cli, env) {
        Ok(out) => (out, 0),
        Err(e) => (e.to_string(), 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn qw(args: &[&str]) -> (String, i32) {
        run(std::iter::once("qw").chain(args.iter().copied()))
    }

    #[test]
    fn ranges_resolve_to_one_based_pages() {
        assert_eq!(
            qw(&["ranges", "1,3,5-6,last", "--pages", "8"]),
            ("1,3,5,6,8".to_owned(), 0)
        );
        assert_eq!(qw(&["ranges", "even", "--pages", "6"]).0, "2,4,6");
    }

    #[test]
    fn range_errors_carry_catalog_codes_and_exit_1() {
        let (out, code) = qw(&["ranges", "11", "--pages", "10"]);
        assert_eq!(code, 1);
        assert!(out.starts_with("QW-RANGE-OUT-OF-BOUNDS"), "{out}");
        let (out, code) = qw(&["ranges", "5-3", "--pages", "10"]);
        assert_eq!((code, out.starts_with("QW-RANGE-SYNTAX")), (1, true));
    }

    #[test]
    fn duplicates_need_the_flag() {
        assert_eq!(qw(&["ranges", "1,1", "--pages", "3"]).1, 1);
        assert_eq!(
            qw(&["ranges", "1,1", "--pages", "3", "--allow-duplicates"]),
            ("1,1".to_owned(), 0)
        );
    }

    #[test]
    fn name_preview_uses_the_pattern() {
        let (out, code) = qw(&["name", "--filename", "report", "--tool", "compressed"]);
        assert_eq!((out.as_str(), code), ("report (compressed)", 0));
        let (out, _) = qw(&[
            "name",
            "--pattern",
            "{filename}-{n:000}",
            "--filename",
            "a",
            "--n",
            "7",
        ]);
        assert_eq!(out, "a-007");
        let (out, code) = qw(&["name", "--pattern", "{page}", "--filename", "a"]);
        assert_eq!(code, 1);
        assert!(out.starts_with("QW-OPTION-INVALID"));
    }

    #[test]
    fn path_policy_is_exposed() {
        assert_eq!(qw(&["path", r"C:\docs\a.pdf"]), ("allowed".to_owned(), 0));
        assert_eq!(qw(&["path", r"\\.\PhysicalDrive0"]).1, 1);
        assert_eq!(qw(&["path", r"\\srv\share\a.pdf"]).1, 1);
        assert_eq!(qw(&["path", r"\\srv\share\a.pdf", "--user-picked"]).1, 0);
    }

    #[test]
    fn empty_registry_lists_no_tools_and_unknown_schema_fails() {
        assert_eq!(qw(&["tools", "list"]).0, "[]");
        let (out, code) = qw(&["tools", "schema", "rotate"]);
        assert_eq!(code, 1);
        assert!(out.starts_with("QW-COMPONENT-MISSING"));
        let (out, code) = qw(&["tools", "schema", "nope"]);
        assert_eq!(code, 1);
        assert!(out.starts_with("QW-OPTION-INVALID"));
    }

    #[test]
    fn bad_usage_exits_2_and_version_works() {
        assert_eq!(qw(&["ranges"]).1, 2);
        assert_eq!(qw(&["version"]).1, 0);
    }
}
