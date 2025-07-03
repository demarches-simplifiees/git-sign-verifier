# git-sign-verifier

This program verifies that commits in a git repository have been signed authorized keys. This helps to prevent malicious commits to be deployed.

## How it works

A reference tag marks the commit from which the verification process begins; this is either the initial commit for verification or the last successfully verified commit. A verification involves checking that every commit since this tag is signed with an authorized key.


## Prequisites

### Git user config

The reference tag is signed with the user local config of the repo. This means that before initialization, the repo copy which is used for verification must be configured with a valid user name and email :

```sh
git config --local user.name "John Doe"
git config --local user.email "john.doe@example.com"
```

### GPG keyring

Authorized public keys are sourced from the `.gpg_authorized_keys` file within the repository. During execution, these keys are imported into the GPG keyring of the user running the program. This keyring must also contain the secret key used to sign the verification tag. The authorization of keys is based on the contents of this file at the commit referenced by the latest verification tag.

If you want to use a specific gpg keyring, you can specify it with the `--gpgme-home-dir` option below.


## Actions

### `init`

Initializes the repository for commit signature verification. This action sets up a reference tag, named `SIGN_VERIFIED`, pointing to the latest commit on the `main` branch. This tag serves as the starting point for future verification runs.

If you want to use a specific gpg keyring for verifications, you can specify it with the `--gpgme-home-dir` option.

Note that the `.gpg_authorized_keys` file must exist in the repository at the time of initialization.

**Usage:**

```bash
git-sign-verifier init # default to current directory
git-sign-verifier init --directory /path/to/your/repo
git-sign-verifier init --gpgme-home-dir /path/to/authorized/gpg/keyring # default to ~/.gnupg
```

### `verify`

Verifies the commits since the latest commit on the `SIGN_VERIFIED` tag. Tag and commits since this tag must all be signed with known keys.

Currently, only GPG keys are supported. Authorized public keys for commits are imported from `.gpg_authorized_keys`, read on the tag commit and the are imported either into the default GPG keyring of the user running the program or from a keyring specified in `init` command. Keys must not have expired or been revoked. Trust level is not supported.

This action will fail if :
- the tag was not signed with a key in the initial keyring
- any commit since this tag was is not signed with an authorized key present either in keyring or in `.gpg_authorized_keys`



**Usage:**

```bash
git-sign-verifier verify
git-sign-verifier verify --directory /path/to/your/repo
```

### Merge commits

A merge commit is considered verified when all the following conditions are met:
- The merge commit itself is signed by an authorized key (for example, GitHub's public key must be authorized when using GitHub). This prevents unauthorized merges from untrusted contributors.
- All parent commits are signed by authorized keys.
- All recursive parent commits have been verified up to the last `SIGN_VERIFIED` tag.

To accept external contributions, every commit must be signed off with an authorized key.

## Tests

Run tests with `RUST_TEST_THREADS=1 cargo test`.

Minimal static git repositories are used for testing. At test time they are extracted from a `tar` archive into a temporary directory so that they can be modified without affecting the original repository.

Because of gpg agent which must run with different GPG home across tests, the tests must be executed sequentially with `RUST_TEST_THREADS=1` env variable.

If you need to update the test repositories, uncompress the `tar`, update it and re-tar the repository with `scripts/compress_tests_repos`.

The keyring used for verification and signing the tag is `tests/fixtures/gpg`.

The secret and public keys used for commits in tests repositories are in `tests/fixtures/user-test-example-keys.asc` file.

## Generate GPG authorized keys file

### Download keys from GitHub users

Generate a `.gpg_authorized_keys` file containing GPG public keys from a list of GitHub users.

List the GitHub usernames in a `gh_users.txt` file (one username per line), then run:

```bash
./scripts/download_gh_users_keys
```

This script downloads public keys from `https://github.com/username.gpg` for each user and consolidates them into a single `gpg_authorized_keys` file with comments indicating the source. Move this file to your repository as `.gpg_authorized_keys` to use it for commit signature verification.
You can include `web-flow` which is the key for GitHub merge commits.
