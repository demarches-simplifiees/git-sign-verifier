use git2::{Error as GitError, Repository};

pub const TAG_NAME: &str = "SIGN_VERIFIED";
pub const EXIT_INVALID_SIGNATURE: i32 = 127;

pub struct Config {
    pub gpgme_home_dir: Option<String>,
}

pub fn read_or_update_local_config(
    repo: &Repository,
    gpgme_home_dir: Option<String>,
) -> Result<Config, GitError> {
    let repo_config = repo.config()?;
    let mut local_config = repo_config.open_level(git2::ConfigLevel::Local)?;

    let resolved_gpgme_home_dir = resolve_gpgme_home_dir(&mut local_config, gpgme_home_dir, repo);

    Ok(Config {
        gpgme_home_dir: resolved_gpgme_home_dir,
    })
}

// gpgme_home_dir is provided as relative path for portability
// but need to work as an absolute path.
fn resolve_gpgme_home_dir(
    local_config: &mut git2::Config,
    gpgme_home_dir: Option<String>,
    repo: &Repository,
) -> Option<String> {
    match gpgme_home_dir {
        Some(dir) => {
            local_config
                .set_str("git-sign-verifier.gpgmehomedir", &dir)
                .unwrap();
            abs_path(&repo, &dir)
        }
        None => match local_config.get_string("git-sign-verifier.gpgmehomedir") {
            Ok(dir) => abs_path(&repo, &dir),
            Err(_) => None, // default home will be used
        },
    }
}

fn abs_path(repo: &Repository, dir: &str) -> Option<String> {
    let abs_path = repo
        .workdir()
        .unwrap()
        .join(dir)
        .to_str()
        .unwrap()
        .to_string();
    Some(abs_path)
}
