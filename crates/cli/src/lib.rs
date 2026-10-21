//! `qw`: headless access to Quirewise rules and the tool registry, for tests and power users.
//!
//! Tool subcommands are added per phase through the registry; nothing here hardcodes tools.
#![forbid(unsafe_code)]

use application::UnitRunner;
use application::ports::{Clock, ComponentInstaller, ComponentStore, FileStore};
use application::query::{DocumentQuery, QueryResult, QueryRunner};
use application::{AppError, CancellationToken, Password, ToolRegistry};
use clap::{Parser, Subcommand};
use domain::naming::{NameContext, OutputNamingPattern};
use domain::path_rules::{UncPolicy, check_path_text};
use domain::{Duplicates, PageRange, ToolId};
use infrastructure::{
    AppPaths, AtomicFileStore, ComponentManager, FsComponentStore, HttpsDownloader, SystemClock,
    WorkerSession, WorkerSupervisor, default_trust,
};
use std::path::PathBuf;
use std::sync::Arc;

mod run_cmd;

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
    /// Run a registered tool on files through the worker.
    Run {
        /// Tool id, for example `rotate` or `split-every-n`. See `qw tools list`.
        tool: String,
        /// Input files. A tool that prints a web address (`html-to-pdf` with `url`) needs none.
        files: Vec<PathBuf>,
        /// Tool options as JSON; missing fields use the defaults, for example `{"angle":"180"}`.
        #[arg(long, conflicts_with = "options_file")]
        options: Option<String>,
        /// Read the options JSON from a file. Use it for secrets such as the passwords of
        /// `protect`, which would otherwise show up in the process list.
        #[arg(long)]
        options_file: Option<PathBuf>,
        /// Output folder. Default: next to each source file.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Read a password from the first line of standard input and use it for every file.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Run an exported recipe (a chain of tools) on files and/or a folder.
    Recipe {
        /// The recipe file exported from the app.
        recipe: PathBuf,
        /// Input files.
        files: Vec<PathBuf>,
        /// Also take every PDF from this folder (Office files too when the recipe converts them).
        #[arg(long)]
        folder: Option<PathBuf>,
        /// Scan the sub-folders of `--folder` as well.
        #[arg(long, requires = "folder")]
        recursive: bool,
        /// Output folder. Default: next to each source file.
        #[arg(long)]
        out: Option<PathBuf>,
        /// Read a password from the first line of standard input and use it for every file.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Print the JSON Schema of the types the UI exchanges with the core.
    Schema,
    /// List the certificates of the Windows store that can sign, for `digital-sign`.
    Certificates,
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
    /// List, install, import and remove on-demand components such as LibreOffice.
    Components {
        /// Component action.
        #[command(subcommand)]
        action: ComponentsAction,
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

/// Component actions.
#[derive(Debug, Subcommand)]
pub enum ComponentsAction {
    /// List the components installed on this computer, as JSON.
    List,
    /// List the components that can be installed. Uses the internet, so it needs `--yes`.
    Available {
        /// Agree to contact the component server.
        #[arg(long)]
        yes: bool,
    },
    /// Download and install a component. Uses the internet, so it needs `--yes`.
    Install {
        /// Component id, for example `libreoffice`.
        id: String,
        /// Agree to download the component.
        #[arg(long)]
        yes: bool,
    },
    /// Install a component from a `.qwpkg` file.
    Import {
        /// The package file.
        file: PathBuf,
    },
    /// Remove a component.
    Remove {
        /// Component id.
        id: String,
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
    /// Runs tool units, normally through the worker process.
    pub runner: Arc<dyn UnitRunner>,
    /// Installed components.
    pub components: &'a dyn ComponentStore,
    /// Installs, imports and removes components.
    pub installer: &'a dyn ComponentInstaller,
}

/// Runs a command and returns the text to print.
pub fn execute(cli: Cli, env: Env<'_>) -> Result<String, AppError> {
    let Env {
        registry,
        clock,
        queries,
        files,
        password,
        runner,
        components,
        installer,
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
        Command::Run {
            tool,
            files: inputs,
            options,
            options_file,
            out,
            ..
        } => {
            let from_file = options_file
                .map(|path| {
                    std::fs::read_to_string(&path)
                        .map_err(|e| AppError::from_io("read the options file", &e))
                })
                .transpose()?;
            run_cmd::run_tool(
                registry,
                runner,
                run_cmd::RunArgs {
                    tool: &tool,
                    files: &inputs,
                    options: from_file.as_deref().or(options.as_deref()),
                    out,
                    password,
                },
            )
        }
        Command::Recipe {
            recipe,
            files: inputs,
            folder,
            recursive,
            out,
            ..
        } => run_cmd::run_recipe(
            runner,
            run_cmd::RecipeArgs {
                recipe: &recipe,
                files: &inputs,
                folder,
                recursive,
                out,
                password,
            },
        ),
        Command::Schema => Ok(application::dto::ipc_schema_json()),
        Command::Certificates => {
            match queries.query(&DocumentQuery::Certificates, &CancellationToken::new())? {
                QueryResult::Certificates(list) => json(&list),
                _ => Err(AppError::internal("listing the certificates")),
            }
        }
        Command::Inspect { path, .. } => {
            let query = DocumentQuery::Inspect { path, password };
            match queries.query(&query, &CancellationToken::new())? {
                QueryResult::Inspect(report) => json(&*report),
                _ => Err(AppError::internal("inspecting the file")),
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
        Command::Components { action } => match action {
            ComponentsAction::List => {
                let mut list = components.installed()?;
                list.sort_by(|a, b| a.id.cmp(&b.id));
                json(&list)
            }
            ComponentsAction::Available { yes } => {
                require_consent(yes)?;
                json(&installer.available()?)
            }
            ComponentsAction::Install { id, yes } => {
                require_consent(yes)?;
                let entry = installer.install(&id, &application::NoProgress)?;
                Ok(format!("installed {} {}", entry.id, entry.version))
            }
            ComponentsAction::Import { file } => {
                let entry = installer.import(&file)?;
                Ok(format!("installed {} {}", entry.id, entry.version))
            }
            ComponentsAction::Remove { id } => {
                installer.remove(&id)?;
                Ok(format!("removed {id}"))
            }
        },
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

fn require_consent(yes: bool) -> Result<(), AppError> {
    if yes {
        Ok(())
    } else {
        Err(AppError::new(
            domain::ErrorCode::NetworkConsentRequired,
            "This uses the internet. Add --yes to agree.",
        ))
    }
}

/// Builds the registry the CLI describes tools from: the same tools the worker runs.
pub fn default_registry() -> Result<ToolRegistry, AppError> {
    let mut registry = ToolRegistry::new();
    application::tools::register_all(&mut registry, &application::tools::ToolDeps::unavailable())?;
    Ok(registry)
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
    run_with(args, worker_exe())
}

/// Like [`run`], with an explicit worker executable. Tests use it to run against a freshly built
/// worker.
pub fn run_with<I, T>(args: I, worker: PathBuf) -> (String, i32)
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
        | Command::Run {
            password_stdin: true,
            ..
        }
        | Command::Recipe {
            password_stdin: true,
            ..
        }
        | Command::Thumbnail {
            password_stdin: true,
            ..
        } => read_password_line(),
        _ => None,
    };
    let limits = domain::limits::SafetyLimits::default();
    let session = WorkerSession::new(worker.clone(), limits);
    let supervisor: Arc<dyn UnitRunner> = Arc::new(WorkerSupervisor::new(worker, limits));
    let registry = match default_registry() {
        Ok(registry) => registry,
        Err(e) => return (e.to_string(), 1),
    };
    let components_dir = AppPaths::from_environment()
        .map(|p| p.components_dir)
        .unwrap_or_default();
    let components = FsComponentStore::new(components_dir.clone());
    let installer = ComponentManager::new(
        components_dir,
        Arc::new(HttpsDownloader::new()),
        default_trust(),
        ComponentManager::current_arch(),
    );
    let env = Env {
        registry: &registry,
        clock: &SystemClock,
        queries: &session,
        files: &AtomicFileStore,
        password,
        runner: supervisor,
        components: &components,
        installer: &installer,
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
    fn components_need_consent_before_the_internet_is_used() {
        for args in [
            &["components", "available"][..],
            &["components", "install", "libreoffice"][..],
        ] {
            let (out, code) = qw(args);
            assert_eq!(code, 1, "{args:?}");
            assert!(out.starts_with("QW-NETWORK-CONSENT-REQUIRED"), "{out}");
        }
    }

    #[test]
    fn listing_components_works_offline_and_removing_an_unknown_one_is_harmless() {
        let (out, code) = qw(&["components", "list"]);
        assert_eq!(code, 0, "{out}");
        assert!(out.trim_start().starts_with('['), "{out}");
        assert_eq!(
            qw(&["components", "remove", "not-installed"]).0,
            "removed not-installed"
        );
        let (out, code) = qw(&["components", "remove", "../x"]);
        assert_eq!(code, 1);
        assert!(out.starts_with("QW-OPTION-INVALID"), "{out}");
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
    fn the_registry_lists_every_tool_and_unknown_schemas_fail() {
        let (list, code) = qw(&["tools", "list"]);
        assert_eq!(code, 0);
        let tools: serde_json::Value = serde_json::from_str(&list).unwrap();
        assert_eq!(
            tools.as_array().map(Vec::len),
            Some(domain::ToolId::ALL.len())
        );
        let (schema, code) = qw(&["tools", "schema", "rotate"]);
        assert_eq!(code, 0);
        assert!(schema.contains("angle"));
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
