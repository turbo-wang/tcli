use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn help_shows_wallet_and_request() {
    let mut cmd = Command::cargo_bin("tcli").unwrap();
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stdout(contains("wallet"))
        .stdout(contains("request"));
}

#[test]
fn guide_runs() {
    let mut cmd = Command::cargo_bin("tcli").unwrap();
    cmd.arg("guide");
    cmd.assert().success().stdout(contains("tempo"));
}

#[test]
fn unknown_command_hint() {
    let mut cmd = Command::cargo_bin("tcli").unwrap();
    cmd.arg("not-a-real-subcommand");
    cmd.assert()
        .failure()
        .stderr(contains("unknown command"))
        .stderr(contains("tcli add"));
}
