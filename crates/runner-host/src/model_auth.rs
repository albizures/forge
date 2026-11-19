//! Deliberately selected local model credentials, separate from Forge capability delivery.
use forge_app::RegistrationError;
use std::{io::Read, path::Path};

/// Validate the sole initially supported host authentication mechanism: a raw
/// `OpenAI` API key in an explicit canonical 0600 file under a canonical 0700
/// directory owned by the daemon user. Never discover HOME, Pi settings or env.
/// This checks local configuration, not provider permissions or account validity.
/// Call only for new-launch preflight, never historical inspection/retirement.
/// # Errors
/// Missing, redirected, permissive, nonregular or malformed credentials reject
/// with a fixed diagnostic that includes neither path nor credential contents.
pub fn preflight(path: &Path) -> Result<(), RegistrationError> {
    let check = || -> std::io::Result<()> {
        if !path.is_absolute() || path.canonicalize()? != path {
            return Err(super::invalid());
        }
        super::directory(path.parent().ok_or_else(super::invalid)?)?;
        let mut bytes = Vec::new();
        super::protected_file(path)?
            .take(4098)
            .read_to_end(&mut bytes)?;
        // Permit one final LF for ordinary secret-file creation; otherwise one
        // nonblank printable ASCII token. No credential-store/OAuth parsing.
        let token = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
        if token.is_empty()
            || token.len() > 4096
            || !token.iter().all(|b| (0x21..=0x7e).contains(b))
        {
            return Err(super::invalid());
        }
        Ok(())
    };
    check().map_err(|_| {
        RegistrationError::new(
            "durable-model-auth-unavailable",
            "configure an explicit owner-only OpenAI API-key file in an owner-only directory",
        )
    })
}
