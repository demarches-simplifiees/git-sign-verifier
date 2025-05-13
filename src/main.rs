use clap::{Parser, Subcommand};
use git2::{Commit, Error as GitError, Reference, Repository};

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
    },

    /// Verify the commits since last tags are signed with authenticated signing keys.
    Verify {
        /// Path of repository
        #[arg(short, long, default_value = ".")]
        directory: String,
    },
}

const TAG_NAME: &str = "SIGN_VERIFIED";

struct TaggerConfig {
    name: String,
    email: String,
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

fn add_tag(
    repo: &Repository,
    commit: &Commit,
    tagger_config: &TaggerConfig,
) -> Result<(), GitError> {
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
) -> Result<TaggerConfig, GitError> {
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

    Ok(TaggerConfig {
        name: resolved_name,
        email: resolved_email,
    })
}

fn verify_from_ref(
    repo: &Repository,
    from_ref: &Reference,
    to_ref: &Reference,
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

    for oid in commits {
        let commit = repo.find_commit(oid.unwrap())?;
        match commit.header_field_bytes("gpgsig") {
            Ok(_signature_data) => {
                println!("Commit {} GPG signature found", commit.id());
            }
            Err(_) => {
                eprintln!("Commit {} is not signed!", commit.id());
                std::process::exit(1);
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

            let local_config = read_or_update_local_config(&repo, None, None).unwrap();

            match verify_from_ref(&repo, &from_ref, &to_ref) {
                Ok(()) => {
                    println!("All commits were signed.");
                    let to_commit = to_ref.peel_to_commit().unwrap();
                    match add_tag(&repo, &to_commit, &local_config) {
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
