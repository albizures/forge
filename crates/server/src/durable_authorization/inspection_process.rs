//! Bounded read-only component transport, not Assignment control or authority.
use std::{io, path::Path, process::Stdio, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn unavailable() -> io::Error {
    io::Error::other("read-only inspector unavailable")
}

pub(super) async fn read(
    node: &Path,
    inspector: &Path,
    directory: &Path,
    input: &[u8],
    deadline: Duration,
    max_response: u64,
) -> io::Result<Vec<u8>> {
    let limit = max_response.checked_add(1).ok_or_else(unavailable)?;
    let mut child = tokio::process::Command::new(node)
        .arg(inspector)
        .arg(directory)
        // Exclude NODE_OPTIONS, ambient Pi discovery and credentials.
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let result = tokio::time::timeout(deadline, async {
        let mut stdin = child.stdin.take().ok_or_else(unavailable)?;
        let stdout = child.stdout.take().ok_or_else(unavailable)?;
        stdin.write_all(input).await?;
        drop(stdin);
        let mut bytes = Vec::new();
        stdout.take(limit).read_to_end(&mut bytes).await?;
        if bytes.len() as u64 > max_response || !child.wait().await?.success() {
            return Err(unavailable());
        }
        Ok(bytes)
    })
    .await;
    if let Ok(Ok(bytes)) = result {
        return Ok(bytes);
    }
    // Reap only this directly spawned inspector. Kernel I/O may be stuck, so
    // cleanup cannot turn the observation deadline into an unbounded wait.
    // This is never proof of Assignment/descendant cleanup or process absence.
    tokio::time::timeout(Duration::from_secs(1), child.kill())
        .await
        .map_err(|_| unavailable())??;
    Err(unavailable())
}
