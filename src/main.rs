use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialise le dépôt en crééant le tag de référence sur le dernier commit de la branche `main`.
    Init {},
}

fn main() {
    let cli = Cli::parse();
}
