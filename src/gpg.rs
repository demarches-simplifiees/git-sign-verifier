use crate::config::Config;
use gpgme::{Context, Protocol, SignatureSummary, VerificationResult};

// Initialize a GPG verification context
pub fn create_gpg_context(config: &Config) -> gpgme::Context {
    let mut gpg_ctx = match Context::from_protocol(Protocol::OpenPgp) {
        Ok(ctx) => ctx,
        Err(e) => {
            panic!("Error while initializing GPGME context: {}", e);
        }
    };

    if let Some(home_dir) = config.gpgme_home_dir.as_ref() {
        if let Err(e) = gpg_ctx.set_engine_home_dir(home_dir.as_str()) {
            panic!("Error setting GPGME home directory: {}", e);
        }
    }

    gpg_ctx
}

// Verify a message has been signed by a known key.
// A single valid is enough so
// we have to ignore errors on any other signature
// until we eventually find a signature from a known key.
//
// See https://github.com/gpg-rs/gpgme/blob/master/examples/verify.rs
pub fn verify_gpg_signature_result(
    verification_result: VerificationResult,
) -> Result<(), &'static str> {
    let mut errors = Vec::new();

    for sig in verification_result.signatures() {
        let fingerprint = sig.fingerprint().unwrap();
        println!("   Verify key {}", fingerprint);

        if sig.summary().contains(SignatureSummary::KEY_REVOKED) {
            errors.push("GPG key revoked");
        }

        if sig.summary().contains(SignatureSummary::KEY_EXPIRED) {
            errors.push("GPG key expired");
        }

        if sig.summary().contains(SignatureSummary::SIG_EXPIRED) {
            errors.push("Signature expired");
        }

        if sig.summary().contains(SignatureSummary::KEY_MISSING) {
            errors.push("Unknown GPG key, missing in keyring");
        }

        if errors.is_empty() {
            return Ok(());
        }
    }

    if errors.is_empty() {
        Err("No signature found")
    } else {
        Err(errors.first().unwrap())
    }
}
