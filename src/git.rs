use crate::config::TAG_NAME;
use git2::{Commit, Error as GitError, Reference, Repository};

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

    repo.tag(
        TAG_NAME,
        commit.as_object(),
        &tagger,
        "Verification tag managed by git-sign-verifier",
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
