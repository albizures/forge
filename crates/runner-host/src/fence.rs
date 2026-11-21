//! Watchdog fence markers and the original host's post-fence quiescence receipt.
//!
//! A watchdog fence (lease expiry, clock or owner uncertainty, denied control)
//! writes a marker that blocks every new grant. The original dedicated subreaper
//! may later prove that its directly spawned worker was positively reaped and
//! that the kernel reports no remaining descendants (ECHILD). Only then does it
//! write one protected, exact receipt bound to the marker, the owner and the last
//! accepted lease, and release ownership by exiting. Without that proof the hold
//! is retained indefinitely, as before. Nothing here adopts or signals a process.
use crate::{directory, invalid, protected_file};
use forge_app::authorization_lease::{Lease, Owner};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Read},
    os::unix::fs::MetadataExt,
    path::Path,
};

/// The first fence marker of a runtime directory.
pub(crate) const MARKER: &str = "watchdog.json";
const RECEIPT_PREFIX: &str = "watchdog-receipt-";
const UNGRANTED_PREFIX: &str = "ungranted-receipt-";
const MAX_BYTES: u64 = 65_536;

/// The per-episode marker used when an earlier episode's `watchdog.json` exists.
pub(crate) fn episode_marker(owner: &Owner) -> String {
    format!(
        "watchdog-{}-{}-fence.json",
        owner.host_pid, owner.host_start
    )
}

fn receipt_name(lease: &Lease) -> String {
    format!("{RECEIPT_PREFIX}{}.json", lease.sequence)
}

/// A canonical decimal, as the purge classifier also requires.
fn canonical(digits: &str) -> bool {
    !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
}

/// `watchdog.json` or `watchdog-<host pid>-<host start>-fence.json`.
#[must_use]
pub fn is_marker(name: &str) -> bool {
    name == MARKER
        || name
            .strip_prefix("watchdog-")
            .and_then(|rest| rest.strip_suffix("-fence.json"))
            .and_then(|rest| rest.split_once('-'))
            .is_some_and(|(pid, start)| {
                canonical(pid)
                    && pid != "0"
                    && pid.parse::<u32>().is_ok()
                    && canonical(start)
                    && start.parse::<u64>().is_ok()
            })
}

fn ungranted_name(owner: &Owner) -> String {
    format!(
        "{UNGRANTED_PREFIX}{}-{}.json",
        owner.host_pid, owner.host_start
    )
}

/// `ungranted-receipt-<host pid>-<host start>.json`: proof from a host whose
/// fence came before any accepted grant (T26).
#[must_use]
pub fn is_ungranted_receipt(name: &str) -> bool {
    name.strip_prefix(UNGRANTED_PREFIX)
        .and_then(|rest| rest.strip_suffix(".json"))
        .and_then(|rest| rest.split_once('-'))
        .is_some_and(|(pid, start)| {
            canonical(pid)
                && pid != "0"
                && pid.parse::<u32>().is_ok()
                && canonical(start)
                && start.parse::<u64>().is_ok()
        })
}

/// `watchdog-receipt-<lease sequence>.json`.
#[must_use]
pub fn is_receipt(name: &str) -> bool {
    name.strip_prefix(RECEIPT_PREFIX)
        .and_then(|rest| rest.strip_suffix(".json"))
        .is_some_and(|sequence| {
            canonical(sequence) && sequence != "0" && sequence.parse::<u32>().is_ok()
        })
}

/// Protected post-fence proof. `marker_contents` is the exact marker file.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub version: u32,
    pub marker: String,
    pub marker_contents: String,
    pub lease: Lease,
    pub children_absent: bool,
}

/// Protected post-fence proof from a host that never accepted any grant: the
/// fence marker names this owner and no last lease. It proves only that this
/// host's worker and every descendant are gone; it never authorizes anything.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UngrantedReceipt {
    pub version: u32,
    pub marker: String,
    pub marker_contents: String,
    pub owner: Owner,
    pub children_absent: bool,
}

#[derive(Deserialize)]
struct Marker {
    owner: Owner,
    last_lease: Option<Lease>,
}

fn read(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    protected_file(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(invalid());
    }
    Ok(bytes)
}

/// The marker must name exactly this owner and lease; its bytes must be unchanged.
fn bound(directory: &Path, receipt: &Receipt) -> io::Result<()> {
    if receipt.version != 1 || !receipt.children_absent || !is_marker(&receipt.marker) {
        return Err(invalid());
    }
    let bytes = read(&directory.join(&receipt.marker))?;
    if bytes != receipt.marker_contents.as_bytes() {
        return Err(invalid());
    }
    let marker: Marker = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if marker.owner != receipt.lease.owner || marker.last_lease.as_ref() != Some(&receipt.lease) {
        return Err(invalid());
    }
    Ok(())
}

/// The ungranted marker must name exactly this owner and no lease, unchanged.
fn bound_ungranted(directory: &Path, receipt: &UngrantedReceipt) -> io::Result<()> {
    if receipt.version != 1 || !receipt.children_absent || !is_marker(&receipt.marker) {
        return Err(invalid());
    }
    let bytes = read(&directory.join(&receipt.marker))?;
    if bytes != receipt.marker_contents.as_bytes() {
        return Err(invalid());
    }
    let marker: Marker = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if marker.owner != receipt.owner || marker.last_lease.is_some() {
        return Err(invalid());
    }
    Ok(())
}

/// Written only by the original supervisor whose policy never accepted a grant,
/// after `Supervised::fenced_quiescence`.
pub(crate) fn record_ungranted(directory: &Path, marker: &str, owner: &Owner) -> io::Result<()> {
    let contents = String::from_utf8(read(&directory.join(marker))?).map_err(|_| invalid())?;
    let receipt = UngrantedReceipt {
        version: 1,
        marker: marker.into(),
        marker_contents: contents,
        owner: owner.clone(),
        children_absent: true,
    };
    bound_ungranted(directory, &receipt)?;
    crate::retirement::persist(directory, &ungranted_name(owner), &receipt)
}

/// Read the exact ungranted receipt of the host recorded in protected
/// `owner.json`, under separately held exclusive inspection. The receipt owner
/// must be that recorded host and worker, and the lock identity the owner's.
/// Proves only this host's quiescence before any grant; it authorizes nothing.
/// # Errors
/// Missing, partial, foreign-owner or tampered evidence, or a changed marker.
pub fn ungranted_receipt(directory: &Path) -> io::Result<UngrantedReceipt> {
    crate::directory(directory)?;
    let recorded = crate::record(directory)?;
    let host = &recorded.owner;
    let lock = protected_file(&directory.join("owner.lock"))?.metadata()?;
    if lock.dev().to_string() != host.lock_device || lock.ino().to_string() != host.lock_inode {
        return Err(invalid());
    }
    let name = format!(
        "{UNGRANTED_PREFIX}{}-{}.json",
        host.host_pid, host.host_start
    );
    let receipt: UngrantedReceipt =
        serde_json::from_slice(&read(&directory.join(&name))?).map_err(|_| invalid())?;
    let owner = &receipt.owner;
    if owner.host_pid != host.host_pid
        || owner.host_start != host.host_start
        || owner.boot_id != host.boot_id
        || owner.lock_device != host.lock_device
        || owner.lock_inode != host.lock_inode
        || owner.worker_pid != recorded.worker_pid
        || owner.worker_start != recorded.worker_start
        || name != ungranted_name(owner)
    {
        return Err(invalid());
    }
    bound_ungranted(directory, &receipt)?;
    Ok(receipt)
}

/// The ungranted receipt of exactly this owner, found by the owner's own name
/// and lock identity, never through `owner.json` (a later host overwrites it).
/// # Errors
/// Missing, foreign-owner or tampered evidence, or another lock identity.
pub fn ungranted_receipt_for(directory: &Path, owner: &Owner) -> io::Result<UngrantedReceipt> {
    crate::directory(directory)?;
    let lock = protected_file(&directory.join("owner.lock"))?.metadata()?;
    if lock.dev().to_string() != owner.lock_device || lock.ino().to_string() != owner.lock_inode {
        return Err(invalid());
    }
    let receipt: UngrantedReceipt =
        serde_json::from_slice(&read(&directory.join(ungranted_name(owner)))?)
            .map_err(|_| invalid())?;
    if receipt.owner != *owner {
        return Err(invalid());
    }
    bound_ungranted(directory, &receipt)?;
    Ok(receipt)
}

/// Written only by the original supervisor, after `Supervised::fenced_quiescence`.
pub(crate) fn record(directory: &Path, marker: &str, lease: &Lease) -> io::Result<()> {
    let contents = String::from_utf8(read(&directory.join(marker))?).map_err(|_| invalid())?;
    let receipt = Receipt {
        version: 1,
        marker: marker.into(),
        marker_contents: contents,
        lease: lease.clone(),
        children_absent: true,
    };
    bound(directory, &receipt)?;
    crate::retirement::persist(directory, &receipt_name(lease), &receipt)
}

/// Read the exact receipt for this lease under separately held exclusive
/// inspection. A free lock, an exited host or a marker alone is not proof.
/// # Errors
/// Rejects missing, partial, foreign-owner, other-lease or tampered evidence,
/// a changed marker, and a lock whose identity is not the lease owner's.
pub fn receipt(directory: &Path, lease: &Lease) -> io::Result<Receipt> {
    crate::directory(directory)?;
    let lock = protected_file(&directory.join("owner.lock"))?.metadata()?;
    if lock.dev().to_string() != lease.owner.lock_device
        || lock.ino().to_string() != lease.owner.lock_inode
    {
        return Err(invalid());
    }
    let receipt: Receipt = serde_json::from_slice(&read(&directory.join(receipt_name(lease)))?)
        .map_err(|_| invalid())?;
    if receipt.lease != *lease {
        return Err(invalid());
    }
    bound(directory, &receipt)?;
    Ok(receipt)
}

/// Post-fence proof by the same host for an earlier (or this) lease of the same
/// Launch. The fence marker records the last lease the host accepted, under
/// the same lock that sets the fault, and a fenced host accepts no further
/// lease: any later lease Forge committed was never accepted (T18 F1).
/// # Errors
/// No such receipt, or any unsafe or tampered evidence.
pub fn receipt_for_owner(directory: &Path, last: &Lease) -> io::Result<Receipt> {
    crate::directory(directory)?;
    for name in names(directory)?.into_iter().filter(|n| is_receipt(n)) {
        let Ok(receipt) = serde_json::from_slice::<Receipt>(&read(&directory.join(&name))?) else {
            continue;
        };
        if receipt.lease.owner == last.owner
            && receipt.lease.launch == last.launch
            && receipt.lease.sequence <= last.sequence
        {
            return self::receipt(directory, &receipt.lease);
        }
    }
    Err(invalid())
}

fn names(directory: &Path) -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(directory)? {
        names.push(entry?.file_name().into_string().map_err(|_| invalid())?);
    }
    names.sort();
    Ok(names)
}

/// Fence markers currently present in the directory.
/// # Errors
/// Unreadable directories are uncertainty.
pub fn markers(directory: &Path) -> io::Result<Vec<String>> {
    Ok(names(directory)?
        .into_iter()
        .filter(|name| is_marker(name))
        .collect())
}

/// Every fence marker is explained by a valid receipt from its own episode.
/// A settled marker is history; it does not authorize anything by itself.
/// # Errors
/// Any unsettled, unreadable or tampered marker/receipt is uncertainty.
pub fn settled(directory: &Path) -> io::Result<()> {
    directory_checked(directory)?;
    let names = names(directory)?;
    let mut receipts = Vec::new();
    for name in names.iter().filter(|name| is_receipt(name)) {
        let receipt: Receipt =
            serde_json::from_slice(&read(&directory.join(name))?).map_err(|_| invalid())?;
        if *name != receipt_name(&receipt.lease) {
            return Err(invalid());
        }
        bound(directory, &receipt)?;
        receipts.push(receipt.marker);
    }
    for name in names.iter().filter(|name| is_ungranted_receipt(name)) {
        let receipt: UngrantedReceipt =
            serde_json::from_slice(&read(&directory.join(name))?).map_err(|_| invalid())?;
        if *name != ungranted_name(&receipt.owner) {
            return Err(invalid());
        }
        bound_ungranted(directory, &receipt)?;
        receipts.push(receipt.marker);
    }
    for marker in names.iter().filter(|name| is_marker(name)) {
        if !receipts.contains(marker) {
            return Err(invalid());
        }
    }
    Ok(())
}

fn directory_checked(path: &Path) -> io::Result<()> {
    directory(path).map(|_| ())
}

/// Read-only operator diagnosis of a runtime directory. Descriptive only: it
/// never grants, releases capacity or substitutes for exclusive inspection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Diagnosis {
    /// No watchdog fence marker exists.
    Unfenced,
    /// A fence exists and every marker is settled by a valid quiescence receipt.
    Proven,
    /// A fence exists without valid quiescence proof (still waiting, an escaped
    /// child, an unverified signal, or tampered evidence): the hold is retained.
    Unproven,
}

impl Diagnosis {
    #[must_use]
    pub fn of(directory: &Path) -> Self {
        match markers(directory) {
            Ok(markers) if markers.is_empty() => Self::Unfenced,
            Ok(_) if settled(directory).is_ok() => Self::Proven,
            _ => Self::Unproven,
        }
    }
}
