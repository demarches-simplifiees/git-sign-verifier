# git-sign-verifier

This program verifies that commits in a git repository have been signed authorized keys. This helps to prevent malicious commits to be deployed.

## How it works

A reference tag marks the commit from which the verification process begins; this is either the initial commit for verification or the last successfully verified commit. A verification involves checking that every commit since this tag is signed with an authorized key.
