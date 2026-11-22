//! Protected immutable recovery verification evidence. Never reconstructs grants.
use crate::{directory, invalid, protected_file};
use std::{
    io::{self, Read},
    path::Path,
};

fn name(key: &str) -> io::Result<String> {
    if key.len() != 64
        || !key
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(invalid());
    }
    Ok(format!("recovery-proof-{key}.json"))
}

/// Persist one bounded immutable proof per claim, with owner-only/no-follow
/// exclusive creation and fsync. Exact retries never overwrite conflicting proof.
/// Caller must hold verified waiting ownership before and after this I/O.
/// # Errors
/// Rejects unsafe boundaries, partial/conflicting evidence or unavailable storage.
pub fn persist(path: &Path, key: &str, proof: &serde_json::Value) -> io::Result<()> {
    directory(path)?;
    if serde_json::to_vec(proof).map_err(|_| invalid())?.len() > 65_536 {
        return Err(invalid());
    }
    crate::retirement::persist(path, &name(key)?, proof)
}

/// Revalidate exact retained bytes and protected metadata without repairing them.
/// # Errors
/// Missing, changed, oversized or unsafe proof is uncertainty, never permission.
pub fn verify(path: &Path, key: &str, proof: &serde_json::Value) -> io::Result<()> {
    directory(path)?;
    let mut bytes = Vec::new();
    protected_file(&path.join(name(key)?))?
        .take(65_537)
        .read_to_end(&mut bytes)?;
    if bytes != serde_json::to_vec(proof).map_err(|_| invalid())? {
        return Err(invalid());
    }
    Ok(())
}
