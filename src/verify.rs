use crate::config::{TAG_NAME, read_or_update_local_config};
use crate::git::{add_tag, check_tag_exists, open_repo, print_commit};
use crate::gpg::{create_gpg_context, verify_gpg_signature_result};
use git2::{Commit, Error as GitError, ObjectType, Oid, Reference, Repository};
use gpgme::Context;
use std::io::{BufRead, Write};

pub fn verify_command(repo_path: &str) -> Result<bool, GitError> {
    let repo = open_repo(repo_path);
    let config = read_or_update_local_config(&repo, None)?;

    let mut gpg_ctx = create_gpg_context(&config);

    let from_ref = match check_tag_exists(&repo) {
        Some(gitref) => {
            let oid = gitref.target().unwrap();
            match verify_tag(&repo, &mut gpg_ctx, oid) {
                Ok(true) => gitref,
                Ok(false) => return Ok(false),
                Err(e) => return Err(e),
            }
        }
        None => {
            return Err(GitError::from_str(&format!(
                "Tag {} doesn't exist!",
                TAG_NAME
            )));
        }
    };
    let to_ref = repo.head()?;

    let all_valid = verify_from_ref(&repo, &from_ref, &to_ref, &mut gpg_ctx)?;

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
    gpg_ctx: &mut Context,
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

    for oid in commits {
        let commit_oid = oid.unwrap();

        match verify_commit(&repo, gpg_ctx, commit_oid) {
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
            let text_to_verify_data = signed_commit_data(&commit).unwrap();

            match verify_detached_signature(
                signature_str,
                text_to_verify_data,
                gpg_ctx,
                &commit_oid.to_string(),
            ) {
                Ok(true) => Ok(true),
                Ok(false) => {
                    print_commit(&commit);
                    Ok(false)
                }
                Err(e) => Err(e),
            }
        }
        Err(e) => Err(e),
    }
}

fn verify_tag(repo: &Repository, gpg_ctx: &mut Context, oid: Oid) -> Result<bool, GitError> {
    let object = repo.find_object(oid, None)?;

    match object.kind() {
        Some(ObjectType::Tag) => {
            let tag = object.as_tag().unwrap();

            // Get raw tag data from Object Database
            let odb = repo.odb()?;
            let odb_object = odb.read(oid)?;
            let raw_tag_data = odb_object.data();

            // Tag data is structured like this:
            // object 054b5abcdef
            // type commit
            // tag SIGN_VERIFIED
            // tagger Test User <test@example.com> 1750782139 +0200
            //
            // The message text
            // -----BEGIN PGP SIGNATURE-----
            // iQIzBAABCAAdFiEE3MxljV1HemvIj+0nT7hl/bykvMQFAl6+8/0ACgkQT7hl/byk
            // =kw8E
            // -----END PGP SIGNATURE-----

            // Convert raw data to string to find signature
            let raw_tag_str = std::str::from_utf8(raw_tag_data)
                .map_err(|e| GitError::from_str(&format!("Invalid UTF-8 in tag: {}", e)))?;

            if let Some(sig_start_pos) = raw_tag_str.find("-----BEGIN") {
                // Split at signature start
                let (tag_content, signature_data) = raw_tag_str.split_at(sig_start_pos);

                let text_to_verify_data = gpgme::Data::from_bytes(tag_content.as_bytes()).unwrap();

                verify_detached_signature(
                    signature_data,
                    text_to_verify_data,
                    gpg_ctx,
                    &oid.to_string(),
                )
            } else {
                eprintln!(
                    "🔴 Signature not found in annotated tag. {}",
                    tag.message().unwrap_or("")
                );
                Ok(false)
            }
        }
        _ => {
            eprintln!(
                "🔴 Lightweight tag or tag not signed: impossible to verify its authenticity. {}",
                oid
            );
            Ok(false)
        }
    }
}

// Helper function to verify a detached signature
fn verify_detached_signature(
    signature_str: &str,
    text_to_verify_data: gpgme::Data,
    gpg_ctx: &mut Context,
    identifier: &str,
) -> Result<bool, GitError> {
    let signature_begin = signature_str.lines().next().unwrap_or("");

    if signature_begin == "-----BEGIN PGP SIGNATURE-----" {
        match gpg_ctx.verify_detached(signature_str, text_to_verify_data) {
            Ok(verification_result) => match verify_gpg_signature_result(verification_result) {
                Ok(()) => {
                    println!("✅ Ref {} GPG signature is trusted", identifier);
                    Ok(true)
                }
                Err(e) => {
                    eprintln!("🔴 {} GPG signature is invalid: {}", identifier, e);
                    Ok(false)
                }
            },
            Err(e) => {
                eprintln!(
                    "⚠️ Error in GPG signature verification for reference {}. Error: {}",
                    identifier, e
                );
                Ok(false)
            }
        }
    } else if signature_begin == "-----BEGIN SSH SIGNATURE-----" {
        eprintln!("⚠️ Unsupported SSH signature on reference {}", identifier);
        Ok(false)
    } else {
        eprintln!(
            "⚠️ Unknown signature type on reference {}: (first line is `{}`)",
            identifier, signature_begin
        );
        Ok(false)
    }
}
