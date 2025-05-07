# git-sign-verifier

This program verifies that commits in a git repository have been signed authorized keys. This helps to prevent malicious commits to be deployed.

## How it works

A reference tag marks the commit from which the verification process begins; this is either the initial commit for verification or the last successfully verified commit. A verification involves checking that every commit since this tag is signed with an authorized key.

## Actions

### `init`

Initializes the repository for commit signature verification. This action sets up a reference tag, named `SIGN_VERIFIED`, pointing to the latest commit on the `main` branch. This tag serves as the starting point for future verification runs.

**Usage:**

```bash
git-sign-verifier init # default to current directory
git-sign-verifier init --directory /path/to/your/repo
