use crate::config::{TAG_NAME, read_or_update_local_config};
use crate::gpg::create_gpg_context;
use git2::{Commit, Error as GitError, Reference, Repository};
use std::io::{Read, Seek};

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

/// Get file content from a specific commit
pub fn get_file_content_from_commit(
    repo: &Repository,
    commit: &Commit,
    file_path: &str,
) -> Result<Option<Vec<u8>>, GitError> {
    let tree = commit.tree()?;

    match tree.get_path(std::path::Path::new(file_path)) {
        Ok(tree_entry) => {
            let object = tree_entry.to_object(repo)?;
            if let Some(blob) = object.as_blob() {
                Ok(Some(blob.content().to_vec()))
            } else {
                Ok(None) // Path exists but is not a file
            }
        }
        Err(_) => Ok(None), // File not found
    }
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

    // Create the tag content that Git expects for signing
    // // TIP: git cat-file -p SIGNED_TAG outputs a raw tag content with signature
    let tag_content = format!(
        "object {}\ntype commit\ntag {}\ntagger {} <{}> {} {:+05}\n\n{}\n",
        commit.id(),
        TAG_NAME,
        tagger.name().unwrap_or(""),
        tagger.email().unwrap_or(""),
        tagger.when().seconds(),
        tagger.when().offset_minutes() * 100 / 60, // format is +0200
        base_message
    );

    // Sign the tag content
    let signature = match sign_tag_content(&mut gpg_ctx, &tag_content) {
        Ok(sig) => sig,
        Err(e) => {
            eprintln!("⚠️ Failed to sign tag content: {}", e);
            return Err(GitError::from_str("Failed to sign tag"));
        }
    };

    let signed_tag_content = format!("{}{}", tag_content, signature);

    // Create the tag object directly in the Git object database
    // because git2 does not support adding a signed tag with repo.tag()
    let tag_oid = repo
        .odb()?
        .write(git2::ObjectType::Tag, signed_tag_content.as_bytes())?;

    // Create the reference to the tag
    repo.reference(
        &format!("refs/tags/{}", TAG_NAME),
        tag_oid,
        true, // overwrite
        &format!("{} on {}", base_message, commit.id()),
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

// Sign tag content with GPG
fn sign_tag_content(gpg_ctx: &mut gpgme::Context, content: &str) -> Result<String, gpgme::Error> {
    // Create data for signing
    let content_data = gpgme::Data::from_bytes(content.as_bytes())?;
    let mut signature_data = gpgme::Data::new()?;

    // Create detached signature
    gpg_ctx.set_armor(true);
    gpg_ctx.sign_detached(content_data, &mut signature_data)?;

    // Read the signature
    signature_data.seek(std::io::SeekFrom::Start(0))?;
    let mut signature_buffer = Vec::new();
    signature_data.read_to_end(&mut signature_buffer)?;
    let signature_str =
        String::from_utf8(signature_buffer).expect("GPG signature should be valid UTF-8");

    Ok(signature_str)
}
