#[tokio::main]
async fn main() {
    if let Err(e) = tcli::cli::run().await {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
