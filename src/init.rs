use crate::config::{TAG_NAME, read_or_update_local_config};
use crate::git::{add_tag, check_tag_exists, get_last_commit, open_repo, print_commit};
use git2::Error as GitError;

pub fn init_command(repo_path: &str, gpgme_home_dir: Option<String>) -> Result<(), GitError> {
    let repo = open_repo(repo_path);

    if check_tag_exists(&repo).is_some() {
        return Err(GitError::from_str(&format!(
            "Le tag '{}' existe déjà!",
            TAG_NAME
        )));
    }

    read_or_update_local_config(&repo, gpgme_home_dir)?;

    let commit = get_last_commit(&repo)?;
    add_tag(&repo, &commit)?;

    println!("Tag '{}' initialized on commit:", TAG_NAME);
    print_commit(&commit);

    Ok(())
}
