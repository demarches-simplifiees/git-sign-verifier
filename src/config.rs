use git2::{Error as GitError, Repository};

pub const TAG_NAME: &str = "SIGN_VERIFIED";
pub const EXIT_INVALID_SIGNATURE: i32 = 127;

pub struct Config {
    pub name: String,
    pub email: String,
    pub gpgme_home_dir: Option<String>,
}

pub fn read_or_update_local_config(
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
