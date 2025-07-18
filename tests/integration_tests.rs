use git_sign_verifier::{init_command, verify_command};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod helpers;
use helpers::{copy_directory, extract_tar_archive, kill_gpg_agent};

// Test fixture managing a temporary copy of the git repository
struct TestFixture {
    repo_path: PathBuf,
    temp_dir: PathBuf,
    gpg_home: PathBuf,
}

impl TestFixture {
    // Initialize a test with a specific branch from the test repository.
    // We copy the repository and gpg home to a temporary directory
    // so we can modify the repository without affecting the original and maintain tests concurrency.
    fn with_branch(repo_name: &str, branch: &str) -> Self {
        let base_path = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixtures_dir = base_path.join("tests").join("fixtures");
        let tar_archive = fixtures_dir.join(format!("{}.tar", repo_name));
        let gpg_home = fixtures_dir.join("gpg");

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        // Create unique temporary directory for this test
        // Note: full path length of gpg agent socket must be limited to 108 chars.
        let temp_dir = std::env::temp_dir().join(format!("gsv-{}", timestamp));
        let repo_path = temp_dir.join(repo_name);

        println!("Run test in {}", repo_path.to_str().unwrap());

        // Create temp directory
        fs::create_dir_all(&repo_path).expect("Failed to create temp directory");

        // Extract repo from tar archive
        extract_tar_archive(&tar_archive, &temp_dir).expect("Failed to extract repo archive");

        // Copy gpg_home to temp directory because gpg home dir is relative to repository
        let gpg_temp_path = temp_dir.join("gpg");
        copy_directory(&gpg_home, &gpg_temp_path).expect("Failed to copy gpg_home");

        // Checkout the specified branch
        let output = Command::new("git")
            .current_dir(&repo_path)
            .args(&["checkout", branch])
            .output()
            .expect("Failed to checkout branch");

        if !output.status.success() {
            panic!(
                "Failed to checkout branch '{}': {}",
                branch,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        TestFixture {
            repo_path,
            temp_dir: temp_dir.clone(),
            gpg_home: gpg_temp_path,
        }
    }

    // Initialize repo
    fn init(&self, gpgdir: Option<String>) -> Result<(), git2::Error> {
        init_command(self.repo_path.to_str().unwrap(), gpgdir)
    }

    // Verify commits with proper GPG environment
    // In order to sign tags, gpg agent and context must run
    // with a GNUPGHOME pointing to our temporary keyring.
    fn verify(&self) -> Result<bool, git2::Error> {
        let original_gnupg = std::env::var("GNUPGHOME").ok();

        unsafe {
            std::env::set_var("GNUPGHOME", &self.gpg_home);
        }

        let result = verify_command(self.repo_path.to_str().unwrap());

        unsafe {
            match original_gnupg {
                Some(path) => std::env::set_var("GNUPGHOME", path),
                None => std::env::remove_var("GNUPGHOME"),
            }
        }

        result
    }

    // Clean up temporary files and GPG processes
    fn cleanup(self) {
        kill_gpg_agent(&self.gpg_home);

        let _ = fs::remove_dir_all(self.temp_dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // All commits are signed with trusted key
    #[test]
    fn test_all_commits_signed_trusted() {
        let fixture = TestFixture::with_branch("repo-test", "all-signed");

        // Verify commits
        let result = fixture.verify().expect("Verification failed");
        assert!(result, "All commits should be valid");

        fixture.cleanup();
    }

    // Detection of unsigned commit
    #[test]
    fn test_detect_unsigned_commit() {
        let fixture = TestFixture::with_branch("repo-test", "unsigned");

        // Verify commits - should return false
        let result = fixture.verify().expect("Verification process failed");
        assert!(!result, "Verification should fail due to unsigned commit");

        fixture.cleanup();
    }

    // Detection of commit signed with untrusted key
    #[test]
    fn test_detect_untrusted_key() {
        let fixture = TestFixture::with_branch("repo-test", "untrusted-gpg");

        // Verify commits - should return false
        let result = fixture.verify().expect("Verification process failed");
        assert!(
            !result,
            "Verification should fail due to untrusted signature"
        );

        fixture.cleanup();
    }

    // Detection of merge commit signed, but parent untrusted
    #[test]
    fn test_detect_unsigned_parent_in_merge_commit() {
        let fixture = TestFixture::with_branch("repo-test", "merge-untrusted");

        // Verify commits - should return false
        let result = fixture.verify().expect("Verification process failed");
        assert!(
            !result,
            "Verification should fail due to unsigned parent commit in merge"
        );

        fixture.cleanup();
    }

    // Detection of merge commit signed as well parents
    #[test]
    fn test_verify_trusted_merge_commits() {
        let fixture = TestFixture::with_branch("repo-test", "merge-trusted");

        let result = fixture.verify().expect("Verification failed");
        assert!(result, "All commits should be valid");

        fixture.cleanup();
    }

    // Detection of commit signed with SSH does not panic
    #[test]
    fn test_verify_ssh_signature_unsupported() {
        let fixture = TestFixture::with_branch("repo-test", "signed-ssh");

        let result = fixture.verify().expect("Verification failed");
        assert!(
            !result,
            "Commit with SSH signature should fail because it's not supported"
        );

        fixture.cleanup();
    }

    // Fails on tag having an unknown signature
    #[test]
    fn test_verify_fails_on_tag_with_unknown_signature() {
        let fixture = TestFixture::with_branch("repo-tag-unknown-signature", "main");

        let result = fixture.verify().expect("Verification failed");
        assert!(
            !result,
            "Verification tag should fail because it's not signed with a known signature"
        );

        fixture.cleanup();
    }

    // Init command set the tag
    #[test]
    fn test_init_create_tag() {
        let fixture = TestFixture::with_branch("repo-untagged", "main");
        fixture
            .init(Some(fixture.gpg_home.to_str().unwrap().to_string()))
            .expect("Initialization process failed");

        let repo = git2::Repository::open(&fixture.repo_path).expect("Failed to open repo");
        let reference = repo
            .find_reference("refs/tags/SIGN_VERIFIED")
            .expect("Tag should have been created");

        let tag = reference
            .peel_to_tag()
            .expect("Failed to peel to tag object");
        let tag_message = tag.message().unwrap_or("");
        assert!(
            tag_message.contains("-----BEGIN PGP SIGNATURE-----"),
            "Tag message should contain a PGP signature"
        );

        // Verify the tag signature is valid with git
        let output = std::process::Command::new("git")
            .args(&["tag", "-v", "SIGN_VERIFIED"])
            .current_dir(&fixture.repo_path)
            .env("GNUPGHOME", &fixture.gpg_home)
            .output()
            .expect("Failed to execute git tag -v");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "git tag -v should succeed. stderr: {}",
            stderr
        );

        fixture.cleanup();
    }

    // Init command set gpg config
    #[test]
    fn test_init_define_gpg_config() {
        let fixture = TestFixture::with_branch("repo-untagged", "main");
        fixture
            .init(Some(fixture.gpg_home.to_str().unwrap().to_string()))
            .expect("Initialization process failed");

        let repo = git2::Repository::open(&fixture.repo_path).expect("Failed to open repo");
        let repo_config = repo.config().expect("Failed to read config");
        let config = repo_config
            .open_level(git2::ConfigLevel::Local)
            .expect("Failed to open config");

        let config_dir = config
            .get_string("git-sign-verifier.gpgmehomedir")
            .expect("Invalid gpg config");

        let expected_path = format!("{}/gpg", fixture.temp_dir.to_str().unwrap());

        assert_eq!(
            config_dir, expected_path,
            "Git config `git-sign-verifier.gpgmehomedir` does not match"
        );

        fixture.cleanup();
    }

    // Init fails when authorized keys file is missing
    #[test]
    fn test_init_require_authorized_keys() {
        let fixture = TestFixture::with_branch("repo-untagged", "without-authorized-keys");
        let result = fixture
            .init(Some(fixture.gpg_home.to_str().unwrap().to_string()))
            .is_ok();

        assert!(
            !result,
            "Initialization should fail when there are no .gpg_authorized_keys"
        );

        fixture.cleanup();
    }
}
