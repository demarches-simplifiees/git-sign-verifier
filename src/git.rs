use crate::config::{TAG_NAME, read_or_update_local_config};
use crate::gpg::create_gpg_context;
use git2::{Commit, Error as GitError, Reference, Repository};
use std::io::Read;

// Open a git repository
pub fn open_repo(repo_path: &str) -> Repository {
    Repository::open(repo_path)
        .unwrap_or_else(|e| panic!("Erreur lors de l'accès au dépôt : {}", e.message()))
}

// Verify if a tag exists in a repository
pub fn check_tag_exists<'a>(repo: &'a Repository) -> Option<Reference<'a>> {
    match repo.find_reference(&format!("refs/tags/{}", TAG_NAME)) {
        Ok(reference) => Some(reference),
        Err(_) => None,
    }
}

// Returns HEAD commit
pub fn get_last_commit(repo: &Repository) -> Result<Commit, GitError> {
    let head = repo.head()?;
    let commit = head.peel_to_commit()?;

    Ok(commit)
}

// Pretty print a commit
pub fn print_commit(commit: &Commit) -> () {
    println!("  commit {}", commit.id());
    println!(
        "  author: {} <{}>",
        commit.author().name().unwrap_or(""),
        commit.author().email().unwrap_or("")
    );
    println!("\n  {}", commit.message().unwrap_or("no msg"));
}

// Add a tag on a commit using a tagger config
pub fn add_tag(repo: &Repository, commit: &Commit) -> Result<(), GitError> {
    let user = read_user(repo)?;

    let tagger = git2::Signature::now(&user.name, &user.email)?;

    // Get GPG configuration and context for signing
    let config = read_or_update_local_config(repo, None)?;
    let mut gpg_ctx = create_gpg_context(&config);

    let base_message = "Verification tag managed by git-sign-verifier";

    // Sign the tag message
    let signed_message = match sign_tag_message(&mut gpg_ctx, base_message) {
        Ok(msg) => msg,
        Err(e) => {
            eprintln!("⚠️ Failed to sign tag message: {}", e);
            return Err(GitError::from_str("Failed to sign tag"));
        }
    };

    eprintln!("Signed message {}", signed_message);

    repo.tag(
        TAG_NAME,
        commit.as_object(),
        &tagger,
        &signed_message,
        true, // overwrite
    )?;

    Ok(())
}

struct GitUser {
    name: String,
    email: String,
}

fn read_user(repo: &Repository) -> Result<GitUser, GitError> {
    let repo_config = repo.config()?;
    let config = repo_config.open_level(git2::ConfigLevel::Local)?;

    let name = config.get_string("user.name")?;
    let email = config.get_string("user.email")?;

    Ok(GitUser { name, email })
}

// Sign a tag message with GPG
fn sign_tag_message(gpg_ctx: &mut gpgme::Context, message: &str) -> Result<String, gpgme::Error> {
    // Create data for signing
    let message_data = gpgme::Data::from_bytes(message.as_bytes())?;
    let mut signature_data = gpgme::Data::new()?;

    // Create detached signature
    gpg_ctx.set_armor(true);
    gpg_ctx.sign_detached(message_data, &mut signature_data)?;

    // Read the signature
    let mut signature_buffer = Vec::new();
    signature_data.read_to_end(&mut signature_buffer)?;
    let signature_str =
        String::from_utf8(signature_buffer).expect("GPG signature should be valid UTF-8");

    // Combine message and signature
    Ok(format!("{}\n{}", message, signature_str))
}
