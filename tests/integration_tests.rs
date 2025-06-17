use git_sign_verifier::{init_command, verify_command};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

mod helpers;
use helpers::{copy_directory, extract_tar_archive};

// Test fixture managing a temporary copy of the git repository
struct TestFixture {
    repo_path: PathBuf,
    temp_dir: PathBuf,
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
        let temp_dir = std::env::temp_dir().join(format!(
            "git-sign-verifier-{}-{}-{}",
            repo_name, branch, timestamp
        ));
        let repo_path = temp_dir.join(repo_name);

        println!("Run test in {}", repo_path.to_str().unwrap());

        // Create temp directory
        fs::create_dir_all(&repo_path).expect("Failed to create temp directory");

        // Extract repo from tar archive
        extract_tar_archive(&tar_archive, &temp_dir).expect("Failed to extract repo archive");

        // Copy gpg_home to temp directory because gpg home dir is relative to repository
        copy_directory(&gpg_home, &temp_dir.join("gpg")).expect("Failed to copy gpg_home");

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
            temp_dir,
        }
    }

    // Initialize repo
    fn init(&self, gpgdir: Option<String>) -> Result<(), git2::Error> {
        init_command(self.repo_path.to_str().unwrap(), gpgdir)
    }

    // Verify commits
    fn verify(&self) -> Result<bool, git2::Error> {
        verify_command(self.repo_path.to_str().unwrap())
    }

    // Clean up temporary files
    fn cleanup(self) {
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

    // Init command set the tag
    #[test]
    fn test_init_create_tag() {
        let fixture = TestFixture::with_branch("repo-untagged", "main");
        fixture.init(None).expect("Initialization process failed");

        let repo = git2::Repository::open(&fixture.repo_path).expect("Failed to open repo");
        let result = repo.find_reference("refs/tags/SIGN_VERIFIED");

        assert!(result.is_ok(), "Tag should have been created");

        fixture.cleanup();
    }

    // Init command set gpg config
    #[test]
    fn test_init_define_gpg_config() {
        let fixture = TestFixture::with_branch("repo-untagged", "main");
        fixture
            .init(Some("gpgdir".to_string()))
            .expect("Initialization process failed");

        let repo = git2::Repository::open(&fixture.repo_path).expect("Failed to open repo");
        let repo_config = repo.config().expect("Failed to read config");
        let config = repo_config
            .open_level(git2::ConfigLevel::Local)
            .expect("Failed to open config");

        let config_dir = config
            .get_string("git-sign-verifier.gpgmehomedir")
            .expect("Invalid gpg config");

        assert_eq!(
            config_dir, "gpgdir",
            "Git config `git-sign-verifier.gpgmehomedir` does not match"
        );

        fixture.cleanup();
    }
}
