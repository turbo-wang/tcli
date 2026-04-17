use std::ffi::OsString;

use clap::builder::PossibleValuesParser;
use clap::{Parser, Subcommand};

use crate::config::{self, config_path};
use crate::config_file;
use crate::storage::{self, remove_oauth};
use crate::tempo_reference;
use crate::Result;

#[derive(Parser)]
#[command(
    name = "tcli",
    version,
    about = "Agent-oriented CLI: wallet (OAuth), Agentic MPP pay, and HTTP request (402 + Payment retry)."
)]
pub struct CliRoot {
    /// Verbose logging: URLs and response metadata on stderr (like curl -v style for OAuth / HTTP).
    #[arg(short = 'v', long, global = true)]
    pub verbose: bool,
    #[command(subcommand)]
    pub command: TopLevel,
}

#[derive(Subcommand)]
pub enum TopLevel {
    /// Wallet commands (aligned with tempo wallet where applicable; see `tcli guide`)
    Wallet {
        #[command(subcommand)]
        action: WalletAction,
    },
    /// Register a service (stub)
    Add {
        name: String,
    },
    /// Redot Agentic MPP — `POST /api/v1/agentic/mpp/pay` (requires `wallet login`)
    #[command(name = "agentic-mpp")]
    AgenticMpp {
        #[command(subcommand)]
        action: AgenticMppAction,
    },
    /// HTTP request with curl-like flags and 402 demo handling
    Request {
        /// Request URL
        url: String,
        #[arg(short = 'X', long)]
        method: Option<String>,
        #[arg(long)]
        json: Option<String>,
        #[arg(short = 'd', long, action = clap::ArgAction::Append)]
        data: Vec<String>,
        #[arg(short = 'H', long = "header", action = clap::ArgAction::Append)]
        header: Vec<String>,
        #[arg(long)]
        timeout: Option<u64>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        max_spend: Option<String>,
        #[arg(short, long)]
        verbose: bool,
    },
    /// List services (stub)
    List,
    /// Update a service (stub)
    Update {
        name: String,
    },
    /// Remove a service (stub)
    Remove {
        name: String,
    },
    /// Compare tempo vs tcli capabilities
    Guide,
    #[command(external_subcommand)]
    External(Vec<OsString>),
}

#[derive(Subcommand)]
pub enum AgenticMppAction {
    /// Call pay API (prints TypedResult JSON; fails if `data` has no credential)
    Pay {
        /// `AgenticMppPayRequest.amount`
        #[arg(long)]
        amount: f64,
        #[arg(long)]
        challenge_id: Option<String>,
        /// MPP method (`tempo` only in this build; `stripe` and others later).
        #[arg(
            long,
            default_value = "tempo",
            value_parser = PossibleValuesParser::new(["tempo"])
        )]
        method: String,
        #[arg(long, default_value = "usdt")]
        pay_variety_code: String,
        #[arg(long)]
        recipient: Option<String>,
        #[arg(long)]
        token_contract: Option<String>,
        #[arg(short, long)]
        verbose: bool,
    },
}

#[derive(Subcommand)]
pub enum WalletAction {
    /// OAuth2 device flow login (not Tempo passkey wallet — see guide)
    Login {
        /// Internal: poll token endpoint from state written by a parent login (do not use manually).
        #[arg(long, hide = true)]
        poll_state: Option<std::path::PathBuf>,
    },
    /// Remove stored OAuth token
    Logout,
    /// Show OAuth session / readiness (closest to tempo wallet whoami)
    Whoami,
    /// Alias for `whoami` (deprecated)
    Balance,
    /// List access keys (Tempo Wallet only — stub)
    Keys,
    /// Fund wallet: faucet / bridge (Tempo Wallet only — stub)
    Fund,
    /// Transfer tokens (Tempo Wallet only — stub)
    Transfer {
        amount: String,
        token: String,
        to: String,
    },
    /// MPP service directory (Tempo Wallet only — stub)
    Services {
        #[arg(long)]
        search: Option<String>,
        /// Single service id (detail view)
        id: Option<String>,
    },
    /// Payment sessions (MPP — Tempo Wallet only — stub)
    Sessions {
        #[command(subcommand)]
        action: WalletSessionsAction,
    },
    /// Sign MPP payment challenge (Tempo only — stub)
    #[command(name = "mpp-sign")]
    MppSign,
}

#[derive(Subcommand)]
pub enum WalletSessionsAction {
    /// List active payment sessions
    List,
    /// Reconcile local sessions with on-chain state
    Sync,
    /// Close sessions
    Close {
        #[arg(long)]
        all: bool,
        #[arg(long)]
        orphaned: bool,
        #[arg(long)]
        dry_run: bool,
    },
}

pub async fn run() -> Result<()> {
    let cli = CliRoot::parse();
    let home = storage::tcli_home();
    std::fs::create_dir_all(&home).map_err(crate::Error::Io)?;

    let cfg_path = config_path(&home);
    let file_cfg = config_file::load(&cfg_path)?;
    let resolved = config::resolve(&file_cfg)?;

    match cli.command {
        TopLevel::Wallet { action } => match action {
            WalletAction::Login { poll_state } => {
                if let Some(path) = poll_state {
                    crate::auth::login_poll_from_state_file(&path, cli.verbose).await?;
                } else {
                    crate::auth::login(
                        &home,
                        &resolved,
                        cli.verbose,
                        crate::auth::LoginOptions::default(),
                    )
                    .await?;
                }
            }
            WalletAction::Logout => {
                remove_oauth(&home)?;
                eprintln!("Logged out (token removed).");
            }
            WalletAction::Whoami | WalletAction::Balance => {
                crate::wallet_cmd::whoami(&home)?;
            }
            WalletAction::Keys => crate::wallet_cmd::keys()?,
            WalletAction::Fund => crate::wallet_cmd::fund()?,
            WalletAction::Transfer { amount, token, to } => {
                crate::wallet_cmd::transfer(&amount, &token, &to)?;
            }
            WalletAction::Services { search, id } => {
                crate::wallet_cmd::services(&search, &id)?;
            }
            WalletAction::Sessions { action } => match action {
                WalletSessionsAction::List => crate::wallet_cmd::sessions_list()?,
                WalletSessionsAction::Sync => crate::wallet_cmd::sessions_sync()?,
                WalletSessionsAction::Close {
                    all,
                    orphaned,
                    dry_run,
                } => crate::wallet_cmd::sessions_close(all, orphaned, dry_run)?,
            },
            WalletAction::MppSign => crate::wallet_cmd::mpp_sign()?,
        },
        TopLevel::Add { name } => {
            println!(
                "stub: `tcli add` — service manifest download not implemented (name={name:?})."
            );
        }
        TopLevel::AgenticMpp { action } => match action {
            AgenticMppAction::Pay {
                amount,
                challenge_id,
                method,
                pay_variety_code,
                recipient,
                token_contract,
                verbose,
            } => {
                crate::agentic_mpp::run_pay_cli(
                    &home,
                    &resolved,
                    amount,
                    challenge_id,
                    method,
                    pay_variety_code,
                    recipient,
                    token_contract,
                    cli.verbose || verbose,
                )
                .await?;
            }
        },
        TopLevel::Request {
            url,
            method,
            json,
            data,
            header,
            timeout,
            dry_run,
            max_spend,
            verbose,
        } => {
            let args = crate::api::RequestArgs {
                url,
                method,
                json_body: json,
                data_pairs: data,
                headers: header,
                timeout_secs: timeout,
                dry_run,
                max_spend,
                verbose: cli.verbose || verbose,
            };
            crate::api::run_request(&home, &resolved, &args).await?;
        }
        TopLevel::List => {
            println!("stub: `tcli list` — no services registered in this build.");
        }
        TopLevel::Update { name } => {
            println!("stub: `tcli update` — not implemented (name={name:?}).");
        }
        TopLevel::Remove { name } => {
            println!("stub: `tcli remove` — not implemented (name={name:?}).");
        }
        TopLevel::Guide => {
            print!("{}", tempo_reference::guide_text());
        }
        TopLevel::External(args) => {
            let cmd = args
                .first()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            return Err(crate::Error::msg(format!(
                "unknown command `{cmd}`.\n\
                 Hint: install extensions with `tcli add <name>` (see `tcli guide`)."
            )));
        }
    }
    Ok(())
}
