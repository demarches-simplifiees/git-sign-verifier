use clap::{Parser, Subcommand};
use git2::{Commit, Error as GitError, Reference, Repository};
use gpgme::{Context, Protocol};
use std::io::{BufRead, Write};
mod gpg;

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

        /// Name used when setting the git tag. Persisted as local git config 'git-sign-verifier.taggername' for future operations.
        #[arg(long, default_value = "Git Sign Verifier")]
        tagger_name: String,

        /// Email used when setting the git tag. Persisted as local git config 'git-sign-verifier.taggeremail' for future operations.
        #[arg(long, required = true)]
        tagger_email: String,

        /// GPGME home dir, in which trusted public keys are stored (in pubring.kbx file). Default is generally $HOME.
        #[arg(short, long, required = false)]
        gpgme_home_dir: String,
    },

    /// Verify the commits since last tags are signed with authenticated signing keys.
    Verify {
        /// Path of repository
        #[arg(short, long, default_value = ".")]
        directory: String,
    },
}

const TAG_NAME: &str = "SIGN_VERIFIED";
const EXIT_INVALID_SIGNATURE: i32 = 127;

struct Config {
    name: String,
    email: String,
    gpgme_home_dir: Option<String>,
}

fn open_repo(repo_path: String) -> Repository {
    Repository::open(repo_path)
        .unwrap_or_else(|e| panic!("Erreur lors de l'accès au dépôt : {}", e.message()))
}

fn check_tag_exists<'a>(repo: &'a Repository) -> Option<Reference<'a>> {
    match repo.find_reference(&format!("refs/tags/{}", TAG_NAME)) {
        Ok(reference) => Some(reference),
        // TODO: vérifier que le tag est correctement signé
        Err(_) => None,
    }
}

fn get_last_commit(repo: &Repository) -> Result<Commit, GitError> {
    let head = repo.head()?;
    let commit = head.peel_to_commit()?;

    Ok(commit)
}

fn print_commit(commit: &Commit) -> () {
    println!("  commit {}", commit.id());
    println!(
        "  author: {} <{}>",
        commit.author().name().unwrap_or(""),
        commit.author().email().unwrap_or("")
    );
    println!("\n  {}", commit.message().unwrap_or("no msg"));
}

fn add_tag(repo: &Repository, commit: &Commit, tagger_config: &Config) -> Result<(), GitError> {
    let tagger = git2::Signature::now(&tagger_config.name, &tagger_config.email)?;

    repo.tag(
        TAG_NAME,
        commit.as_object(),
        &tagger,
        "Verification tag managed by git-sign-verifier",
        true, // overwrite
    )?;

    Ok(())
}

// Resolve config from local repository and input args.
// Input args are persisted in local git config.
fn read_or_update_local_config(
    repo: &Repository,
    provided_name: Option<String>,
    provided_email: Option<String>,
    gpgme_home_dir: Option<String>,
) -> Result<Config, GitError> {
    let repo_config = repo.config()?;
    let mut local_config = repo_config.open_level(git2::ConfigLevel::Local)?;

    let resolved_name = match provided_name {
        Some(name) => {
            local_config.set_str("git-sign-verifier.taggername", &name)?;
            name
        }
        None => local_config.get_string("git-sign-verifier.taggername")?,
    };

    let resolved_email = match provided_email {
        Some(email) => {
            local_config.set_str("git-sign-verifier.taggeremail", &email)?;
            email
        }
        None => local_config.get_string("git-sign-verifier.taggeremail")?,
    };

    let resolved_gpgme_home_dir = match gpgme_home_dir {
        Some(dir) => {
            local_config.set_str("git-sign-verifier.gpgmehomedir", &dir)?;
            Some(dir)
        }

        None => local_config
            .get_string("git-sign-verifier.gpgmehomedir")
            .ok(),
    };

    Ok(Config {
        name: resolved_name,
        email: resolved_email,
        gpgme_home_dir: resolved_gpgme_home_dir,
    })
}

// In order to verify a signature, we have to construct the payload signed.
// It's composed from the commit headers (except the signature) and the commit message as body.
// Basically we iterate on headers and collect them in a buffer, then we concat the body message.
// Work with bytes to deal with potential encoding issues.
fn signed_commit_data(commit: &Commit) -> gpgme::Result<gpgme::Data<'static>> {
    let raw_header_bytes = commit.raw_header_bytes();
    let mut filtered_header_bytes = Vec::new();

    let mut cursor = std::io::Cursor::new(raw_header_bytes);
    let mut line_buf = Vec::new();
    let mut in_gpgsig_header = false;

    loop {
        line_buf.clear();
        match cursor.read_until(b'\n', &mut line_buf) {
            Ok(0) => break, // End of headers
            Ok(_) => {
                if line_buf.starts_with(b"gpgsig ") {
                    in_gpgsig_header = true;
                } else if in_gpgsig_header && line_buf.starts_with(b" ") {
                    // Content of gpgsig header starts with a space (signature itself)
                } else {
                    // We left gpgsig header
                    if in_gpgsig_header {
                        in_gpgsig_header = false;
                    }

                    filtered_header_bytes
                        .write_all(&line_buf)
                        .unwrap_or_else(|e| {
                            panic!("Error while writing filtered headers: {}", e);
                        });
                }
            }
            Err(e) => {
                panic!("Error while reading commit headers: {}", e);
            }
        };
    }

    let mut payload_to_verify = Vec::new();
    payload_to_verify.extend_from_slice(&filtered_header_bytes);
    payload_to_verify.push(b'\n');
    payload_to_verify.extend_from_slice(commit.message_raw_bytes());

    gpgme::Data::from_bytes(&payload_to_verify)
}

fn verify_from_ref(
    repo: &Repository,
    from_ref: &Reference,
    to_ref: &Reference,
    config: &Config,
) -> Result<(), GitError> {
    let mut commits = repo.revwalk()?;
    let from_oid = from_ref.target().unwrap(); // tag oid
    let from_commit_oid = from_ref.peel_to_commit().unwrap().id(); // commit oid
    let to_oid = to_ref.target().unwrap(); // commit (HEAD) oid

    let range_str = format!("{}..{}", from_oid, to_oid);
    commits.push_range(&range_str)?;
    commits.set_sorting(git2::Sort::REVERSE)?;

    println!(
        "Verifying commits from {from_ref}={from_oid} to {to_ref}={to_oid}",
        from_ref = from_ref.shorthand().unwrap(),
        from_oid = from_commit_oid,
        to_ref = to_ref.shorthand().unwrap(),
        to_oid = to_oid,
    );

    // Initialize a GPG verification context
    let mut gpg_ctx = match Context::from_protocol(Protocol::OpenPgp) {
        Ok(ctx) => ctx,
        Err(e) => {
            panic!("Error while initializing GPGME context: {}", e);
        }
    };

    if let Some(home_dir) = config.gpgme_home_dir.as_ref() {
        if let Err(e) = gpg_ctx.set_engine_home_dir(home_dir.as_str()) {
            panic!("Error setting GPGME home directory: {}", e);
        }
    }

    for oid in commits {
        let commit_oid = oid.unwrap();
        let commit = repo.find_commit(commit_oid)?;

        match commit.header_field_bytes("gpgsig") {
            Ok(signature_data) => {
                let text_to_verify_data = signed_commit_data(&commit).unwrap();

                let verification_result = gpg_ctx
                    .verify_detached(signature_data.as_str().unwrap(), text_to_verify_data)
                    .unwrap();

                match gpg::verify_gpg_signature_result(verification_result) {
                    Ok(()) => {
                        println!("✅ Commit {} GPG signature is trusted", commit_oid);
                    }
                    Err(e) => {
                        eprintln!("🔴 Commit {} GPG signature is invalid: {}", commit_oid, e);
                        std::process::exit(EXIT_INVALID_SIGNATURE);
                    }
                }
            }
            Err(_) => {
                eprintln!("🔴 Commit {} is not signed with GPG", commit_oid);
                std::process::exit(EXIT_INVALID_SIGNATURE);
            }
        }
    }

    Ok(())
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init {
            directory,
            tagger_name,
            tagger_email,
            gpgme_home_dir,
        } => {
            let repo = open_repo(directory);

            match check_tag_exists(&repo) {
                Some(_ref) => {
                    panic!("Le tag '{}' existe déjà!", TAG_NAME);
                }
                None => (),
            }

            let local_config = match read_or_update_local_config(
                &repo,
                Some(tagger_name),
                Some(tagger_email),
                Some(gpgme_home_dir),
            ) {
                Ok(config) => config,
                Err(e) => {
                    eprintln!(
                        "Erreur lors de la lecture/mise à jour de la configuration du tagger: {}",
                        e
                    );
                    std::process::exit(1);
                }
            };

            match get_last_commit(&repo) {
                Ok(commit) => match add_tag(&repo, &commit, &local_config) {
                    Ok(()) => {
                        println!("Tag '{}' initialized on commit:", TAG_NAME);
                        print_commit(&commit);
                    }
                    Err(e) => eprintln!("Erreur lors de la création du tag: {}", e),
                },
                Err(e) => eprintln!("Une erreur est survenue: {}", e),
            }
        }

        Commands::Verify { directory } => {
            let repo = open_repo(directory);

            let from_ref = match check_tag_exists(&repo) {
                Some(gitref) => gitref,
                None => {
                    eprintln!("Tag {} doesn't exist!", TAG_NAME);
                    std::process::exit(1);
                }
            };
            let to_ref = repo.head().unwrap();

            let config = read_or_update_local_config(&repo, None, None, None).unwrap();

            match verify_from_ref(&repo, &from_ref, &to_ref, &config) {
                Ok(()) => {
                    println!("🎉 All commits were signed and trusted.");
                    let to_commit = to_ref.peel_to_commit().unwrap();
                    match add_tag(&repo, &to_commit, &config) {
                        Ok(()) => {
                            println!("Tag {} moved to {}", TAG_NAME, to_commit.id());
                        }
                        Err(e) => eprintln!("Error while creating tag: {}", e),
                    }
                }
                Err(e) => eprintln!("Error while verifying commits: {}", e),
            }
        }
    }
}
