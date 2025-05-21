mod config;
mod git;
mod gpg;
mod init;
mod verify;

use clap::{Parser, Subcommand};
use config::EXIT_INVALID_SIGNATURE;
use init::init_command;
use verify::verify_command;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialise le dépôt en crééant le tag de référence sur le dernier commit de la branche `main`.
    Init {
        /// Path of repository
        #[arg(short, long, default_value = ".")]
        directory: String,

        /// GnuPG home dir (relative path to workdir), in which trusted public keys are stored (in pubring.kbx file).
        #[arg(short, long, required = false)]
        gpgme_home_dir: Option<String>,
    },

    /// Verify the commits since last tags are signed with authenticated signing keys.
    Verify {
        /// Path of repository
        #[arg(short, long, default_value = ".")]
        directory: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init {
            directory,
            gpgme_home_dir,
        } => match init_command(&directory, gpgme_home_dir) {
            Ok(_) => (),
            Err(e) => {
                eprintln!("Erreur lors de l'initialisation: {}", e);
                std::process::exit(1);
            }
        },

        Commands::Verify { directory } => match verify_command(&directory) {
            Ok(valid) => {
                if !valid {
                    std::process::exit(EXIT_INVALID_SIGNATURE);
                }
            }
            Err(e) => {
                eprintln!("Erreur lors de la vérification: {}", e);
                std::process::exit(1);
            }
        },
    }
}
