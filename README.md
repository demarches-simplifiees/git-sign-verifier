# git-sign-verifier

This program verifies that commits in a git repository have been signed authorized keys. This helps to prevent malicious commits to be deployed.

## How it works

A reference tag marks the commit from which the verification process begins; this is either the initial commit for verification or the last successfully verified commit. A verification involves checking that every commit since this tag is signed with an authorized key.

## Actions

### `init`

Initializes the repository for commit signature verification. This action sets up a reference tag, named `SIGN_VERIFIED`, pointing to the latest commit on the `main` branch. This tag serves as the starting point for future verification runs.

If you want to use a specific gpg keyring for verifications, you can specify it with the `--gpgme-home-dir` option.

**Usage:**

```bash
git-sign-verifier init # default to current directory
git-sign-verifier init --directory /path/to/your/repo
git-sign-verifier init --gpgme-home-dir /path/to/authorized/gpg/keyring # default to ~/.gnupg
```

### `verify`

Verifies the commits since the latest commit on the `SIGN_VERIFIED` tag. This action will fail if any commit is not signed with an authorized key.

Currently, only GPG keys are supported. The authorized keys are read either from the default GPG keyring of the user running the program or from a keyring specified in `init` command. Keys must be trusted, and must not have expired or been revoked.


**Usage:**

```bash
git-sign-verifier verify
git-sign-verifier verify --directory /path/to/your/repo
```

### Merge commits

A merge commit is considered verified when :
- merge commit itself comes from an authorized key, i.e. when using github, their pubkey must be authorized. This helps to prevent evil merges with from untrusted contributors.
- all parents commits are signed by an authorized key.
- all recursive parents are verified until the last SIGN_VERIFIED tag.

To accept external contributions, you have to signoff any commit with an authorized key.

## Tests

Run tests with `cargo test`.

Minimal static git repositories are used for testing. At test time they are extracted from a `tar` archive into a temporary directory so that they can be modified without affecting the original repository.

If you need to update the test repositories, uncompress the `tar`, update it and re-tar the repository::

```bash
tar -cf tests/fixtures/repo-untagged.tar -C tests/fixtures repo-untagged
tar -cf tests/fixtures/repo-test.tar -C tests/fixtures repo-test
```
