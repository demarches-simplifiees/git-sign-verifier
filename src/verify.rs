use crate::config::{Config, TAG_NAME, read_or_update_local_config};
use crate::git::{add_tag, check_tag_exists, open_repo, print_commit};
use crate::gpg::verify_gpg_signature_result;
use git2::{Commit, Error as GitError, Oid, Reference, Repository};
use gpgme::{Context, Protocol};
use std::io::{BufRead, Write};

pub fn verify_command(repo_path: &str) -> Result<bool, GitError> {
    let repo = open_repo(repo_path);

    let from_ref = match check_tag_exists(&repo) {
        Some(gitref) => gitref,
        None => {
            return Err(GitError::from_str(&format!(
                "Tag {} doesn't exist!",
                TAG_NAME
            )));
        }
    };
    let to_ref = repo.head()?;

    let config = read_or_update_local_config(&repo, None)?;

    let all_valid = verify_from_ref(&repo, &from_ref, &to_ref, &config)?;

    if all_valid {
        println!("🎉 All commits were signed and trusted.");
        let to_commit = to_ref.peel_to_commit()?;
        add_tag(&repo, &to_commit)?;
        println!("Tag {} moved to {}", TAG_NAME, to_commit.id());
    }

    Ok(all_valid)
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

// Verify all commits are trusted between two references
fn verify_from_ref(
    repo: &Repository,
    from_ref: &Reference,
    to_ref: &Reference,
    config: &Config,
) -> Result<bool, GitError> {
    let mut commits = repo.revwalk()?;
    let from_oid = from_ref.target().unwrap(); // tag oid
    let from_commit_oid = from_ref.peel_to_commit().unwrap().id(); // commit oid
    let to_oid = to_ref.target().unwrap(); // commit (HEAD) oid

    let range_str = format!("{}..{}", from_oid, to_oid);
    commits.push_range(&range_str)?;
    commits.set_sorting(git2::Sort::TOPOLOGICAL)?;
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

        match verify_commit(&repo, &mut gpg_ctx, commit_oid) {
            Ok(true) => continue,
            Ok(false) => return Ok(false),
            Err(_) => {
                eprintln!("🔴 Commit {} is not signed with GPG", commit_oid);
                return Ok(false);
            }
        }
    }

    Ok(true)
}

// Verify signature of a single commit oid given a GPG context
fn verify_commit(
    repo: &Repository,
    gpg_ctx: &mut Context,
    commit_oid: Oid,
) -> Result<bool, GitError> {
    let commit = repo.find_commit(commit_oid)?;
    // Note: GPG and SSH signature are under gpgsig header!
    match commit.header_field_bytes("gpgsig") {
        Ok(signature_data) => {
            let signature_str = signature_data.as_str().unwrap_or("");
            let signature_begin = signature_str.lines().next().unwrap_or("");

            if signature_begin == "-----BEGIN PGP SIGNATURE-----" {
                let text_to_verify_data = signed_commit_data(&commit).unwrap();

                match gpg_ctx.verify_detached(signature_str, text_to_verify_data) {
                    Ok(verification_result) => {
                        match verify_gpg_signature_result(verification_result) {
                            Ok(()) => {
                                println!("✅ Commit {} GPG signature is trusted", commit_oid);
                                Ok(true)
                            }
                            Err(e) => {
                                eprintln!(
                                    "🔴 Commit {} GPG signature is invalid: {}",
                                    commit_oid, e
                                );
                                print_commit(&commit);
                                Ok(false)
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "⚠️ Eroror in GPG signature verification for commit {}. Error: {}",
                            commit_oid, e
                        );
                        Ok(false)
                    }
                }
            } else if signature_begin == "-----BEGIN SSH SIGNATURE-----" {
                eprintln!("⚠️ Unsupported SSH signature on commit {}", commit_oid);
                Ok(false)
            } else {
                eprintln!(
                    "⚠️ Unknown signature type on commit {}: (first line is `{}`)",
                    commit_oid, signature_begin
                );
                Ok(false)
            }
        }
        Err(e) => Err(e),
    }
}
