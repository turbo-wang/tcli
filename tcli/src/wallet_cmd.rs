//! Wallet subcommands: parity with `tempo wallet` CLI surface where possible.
//! On-chain / passkey / MPP features require the official `tempo` binary — see handler text.

use std::path::Path;

use crate::storage::{load_oauth, oauth_path};
use crate::Result;

pub fn whoami(home: &Path) -> Result<()> {
    let path = oauth_path(home);
    match load_oauth(home)? {
        Some(_) => {
            println!("tcli wallet whoami (OAuth2 session — not Tempo passkey wallet)");
            println!("  logged_in: true");
            println!("  ready: true");
            println!("  token_store: {}", path.display());
            println!();
            println!("Official Tempo CLI shows address, balances, and key state:");
            println!("  tempo wallet whoami");
            println!("Docs: https://docs.tempo.xyz/cli/wallet#check-balances");
        }
        None => {
            println!("logged_in: false");
            println!("ready: false");
            println!();
            println!("Run `tcli wallet login`, `testlogin`, or `testcharlogin` for an OAuth demo session, or use `tempo wallet login` for Tempo Wallet.");
        }
    }
    Ok(())
}

pub fn stub_tempo_wallet_only(feature: &str, detail: &str) {
    eprintln!("{feature}");
    eprintln!();
    eprintln!("This requires Tempo Wallet (passkey) and the official Tempo CLI.");
    eprintln!("{detail}");
    eprintln!();
    eprintln!("Docs: https://docs.tempo.xyz/cli/wallet");
    eprintln!("Install: curl -fsSL https://tempo.xyz/install | bash");
}

pub fn keys() -> Result<()> {
    stub_tempo_wallet_only(
        "`tcli wallet keys` — not available in tcli.",
        "Use: tempo wallet keys   (list access keys and spending limits)",
    );
    Ok(())
}

pub fn fund() -> Result<()> {
    stub_tempo_wallet_only(
        "`tcli wallet fund` — not available in tcli.",
        "Use: tempo wallet fund   (testnet faucet / mainnet bridge — see docs)",
    );
    Ok(())
}

pub fn transfer(amount: &str, token: &str, to: &str) -> Result<()> {
    stub_tempo_wallet_only(
        "`tcli wallet transfer` — not available in tcli.",
        &format!(
            "You asked: transfer {amount} {token} → {to}\n\
             Use: tempo wallet transfer <amount> <token> <to>"
        ),
    );
    Ok(())
}

pub fn services(search: &Option<String>, id: &Option<String>) -> Result<()> {
    if search.is_some() && id.is_some() {
        return Err(crate::Error::msg(
            "use either --search or <id>, not both (see tempo wallet services)",
        ));
    }
    stub_tempo_wallet_only(
        "`tcli wallet services` — not available in tcli.",
        &format!(
            "Use: tempo wallet services [--search <query>] [<id>]\n\
             Current args: search={search:?}, id={id:?}"
        ),
    );
    Ok(())
}

pub fn sessions_list() -> Result<()> {
    stub_tempo_wallet_only(
        "`tcli wallet sessions list` — not available in tcli.",
        "Use: tempo wallet sessions list   (MPP payment sessions — see mpp.dev)",
    );
    Ok(())
}

pub fn sessions_sync() -> Result<()> {
    stub_tempo_wallet_only(
        "`tcli wallet sessions sync` — not available in tcli.",
        "Use: tempo wallet sessions sync",
    );
    Ok(())
}

pub fn sessions_close(all: bool, orphaned: bool, dry_run: bool) -> Result<()> {
    if all && orphaned {
        return Err(crate::Error::msg(
            "use only one of --all or --orphaned (see tempo wallet sessions close)",
        ));
    }
    stub_tempo_wallet_only(
        "`tcli wallet sessions close` — not available in tcli.",
        &format!(
            "Use: tempo wallet sessions close [--all|--orphaned] [--dry-run]\n\
             Current flags: --all={all} --orphaned={orphaned} --dry-run={dry_run}"
        ),
    );
    Ok(())
}

pub fn mpp_sign() -> Result<()> {
    stub_tempo_wallet_only(
        "`tcli wallet mpp-sign` — not available in tcli.",
        "Used internally by `tempo request` for MPP. Use: tempo wallet mpp-sign / tempo request",
    );
    Ok(())
}
