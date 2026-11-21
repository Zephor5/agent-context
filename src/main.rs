use agent_context::{Vault, assets, board::Board, lint, mcp, vault::RawRecord};
use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "agent-context",
    version,
    about = "Shared context across agent tools and sessions: a maintained Markdown wiki and a machine-local blackboard."
)]
struct Cli {
    /// Vault directory (or set AGENT_CONTEXT_ROOT)
    #[arg(long, global = true, env = "AGENT_CONTEXT_ROOT")]
    root: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create a new vault from the template and initialise its git repository
    Init { dir: PathBuf },
    /// Run the MCP server over stdio (wiki_capture/search/read, board_list/read/put/edit/delete)
    Serve,
    /// Open the local blackboard and Wiki in your browser (remembers this vault)
    Web {
        /// Serve the same Web API over stdin/stdout for local application integrations
        #[arg(long, conflicts_with = "port")]
        stdio: bool,
        /// Listen on this loopback port; 0 chooses an available port
        #[arg(long, default_value_t = 7420)]
        port: u16,
        /// Print the URL without opening a browser
        #[arg(long)]
        no_open: bool,
    },
    /// List raw records not yet compiled, and remember them as the current batch
    Pending {
        #[arg(long)]
        json: bool,
    },
    /// Commit vault changes and append a line to log.md
    Commit {
        #[arg(short, long)]
        message: String,
        /// Also mark the batch from the last `pending` as compiled and commit those raw files
        #[arg(long)]
        compile: bool,
        #[arg(long)]
        json: bool,
    },
    /// Structural checks: broken links, orphan pages, index gaps, malformed raw headers
    Lint {
        #[arg(long)]
        json: bool,
    },
    /// Print a built-in prompt
    Prompt {
        #[arg(value_enum)]
        name: PromptName,
    },
    /// The machine-local blackboard under board/
    Board {
        #[command(subcommand)]
        cmd: BoardCmd,
    },
}

#[derive(Subcommand)]
enum BoardCmd {
    /// List entries (key, summary, updated_at), newest first
    List {
        /// Keywords that must all appear in the key or value
        query: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// Print one entry
    Read {
        key: String,
        #[arg(long)]
        json: bool,
    },
    /// Create or overwrite an entry; the value is read from stdin unless --value is given
    Put {
        key: String,
        #[arg(long)]
        value: Option<String>,
        /// Only write if the entry is at this version (0 = must not exist)
        #[arg(long)]
        expect_version: Option<u64>,
    },
    /// Replace the one occurrence of --old in an entry with --new
    Edit {
        key: String,
        /// Text to replace; must occur exactly once in the entry
        #[arg(long)]
        old: String,
        /// Replacement text ("" deletes --old)
        #[arg(long)]
        new: String,
        /// Only edit if the entry is at this version
        #[arg(long)]
        expect_version: Option<u64>,
    },
    /// Delete an entry
    Delete { key: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum PromptName {
    Maintainer,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("agent-context: {e:#}");
            ExitCode::from(2)
        }
    }
}

/// `--root` / `AGENT_CONTEXT_ROOT`, else the current directory if it is a vault.
fn open(root: Option<&Path>) -> Result<Vault> {
    if let Some(root) = root {
        return Vault::open(root);
    }
    let cwd = std::env::current_dir()?;
    Vault::open(&cwd).map_err(|_| {
        anyhow::anyhow!(
            "no vault given: pass --root <dir>, set AGENT_CONTEXT_ROOT, or run inside a vault"
        )
    })
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let root = cli.root.as_deref();
    match cli.cmd {
        Cmd::Init { dir } => {
            let (vault, warning) = Vault::init(&dir)?;
            println!("created vault at {}", vault.root().display());
            if let Some(w) = warning {
                eprintln!("warning: {w}");
            }
        }
        Cmd::Serve => {
            let vault = open(root)?;
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?
                .block_on(mcp::serve(vault))?;
        }
        Cmd::Web {
            port,
            no_open,
            stdio,
        } => {
            let vault = agent_context::web::open_vault(root)?;
            if stdio {
                agent_context::web::serve_stdio(vault)?;
            } else {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()?
                    .block_on(agent_context::web::serve(vault, port, !no_open))?;
            }
        }
        Cmd::Pending { json } => {
            let records = open(root)?.pending()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&records)?);
            } else {
                print_pending(&records);
            }
        }
        Cmd::Commit {
            message,
            compile,
            json,
        } => {
            let outcome = open(root)?.commit(&message, compile)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&outcome)?);
            } else if let Some(c) = &outcome.commit {
                println!("committed {c} (raw compiled: {})", outcome.raw_compiled);
            } else {
                println!("nothing to commit");
            }
        }
        Cmd::Lint { json } => {
            let report = lint::lint(&open(root)?)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_lint(&report);
            }
            if !report.ok {
                return Ok(ExitCode::from(1));
            }
        }
        Cmd::Prompt { name } => match name {
            PromptName::Maintainer => print!("{}", assets::MAINTAINER_PROMPT),
        },
        Cmd::Board { cmd } => {
            let vault = open(root)?;
            run_board(&Board::new(&vault), cmd)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn run_board(board: &Board, cmd: BoardCmd) -> Result<()> {
    match cmd {
        BoardCmd::List { query, json } => {
            let q = query.join(" ");
            let entries = board.list(Some(&q))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else if entries.is_empty() {
                println!("board is empty");
            } else {
                for e in &entries {
                    println!("{}  {}  {}", e.updated_at, e.key, e.summary);
                }
            }
        }
        BoardCmd::Read { key, json } => {
            let item = board.read(&key)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&item)?);
            } else {
                println!(
                    "{}  (version {}, updated {})\n\n{}",
                    item.key, item.version, item.updated_at, item.value
                );
            }
        }
        BoardCmd::Put {
            key,
            value,
            expect_version,
        } => {
            let value = match value {
                Some(v) => v,
                None => {
                    let mut s = String::new();
                    std::io::stdin().read_to_string(&mut s)?;
                    s
                }
            };
            let w = board.put(&key, &value, expect_version)?;
            println!("{} -> version {}", w.key, w.version);
        }
        BoardCmd::Edit {
            key,
            old,
            new,
            expect_version,
        } => {
            let w = board.edit(&key, &old, &new, expect_version)?;
            println!("{} -> version {}", w.key, w.version);
        }
        BoardCmd::Delete { key } => {
            let d = board.delete(&key)?;
            if d.deleted {
                println!("deleted {}", d.key);
            } else {
                println!("no entry {}", d.key);
            }
        }
    }
    Ok(())
}

fn print_pending(records: &[RawRecord]) {
    if records.is_empty() {
        println!("no pending raw records");
        return;
    }
    for r in records {
        println!(
            "{}  [{}] {}",
            r.path,
            r.kind.as_deref().unwrap_or("?"),
            r.title.as_deref().unwrap_or("(untitled)")
        );
    }
    println!("{} pending", records.len());
}

fn print_lint(r: &lint::Report) {
    if r.ok {
        println!("ok: no structural issues");
        return;
    }
    for l in &r.broken_links {
        println!("broken link   {} -> [[{}]]", l.page, l.target);
    }
    for p in &r.orphans {
        println!("orphan page   {p}");
    }
    for p in &r.missing_from_index {
        println!("not in index  {p}");
    }
    for i in &r.invalid_raw {
        println!("invalid raw   {}: {}", i.path, i.problem);
    }
    println!("{} issue(s)", r.issue_count());
}
