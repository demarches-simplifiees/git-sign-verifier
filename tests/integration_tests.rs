use git_sign_verifier::verify_command;
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
}
