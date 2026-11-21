//! Classification of a captured handle's current sample; process capture/flock
//! are separately exercised by real supervisors. No grants/signals are produced.
use super::*;
use std::{
    os::unix::fs::PermissionsExt,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

#[test]
fn reaped_child_can_complete_current_sample_but_never_restore_a_previous_fault() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    for fault in 0..4 {
        let path = std::env::temp_dir().join(format!(
            "forge-crash-sample-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(path.join("owner.lock"), "").unwrap();
        fs::set_permissions(path.join("owner.lock"), fs::Permissions::from_mode(0o600)).unwrap();
        let mut child = if fault == 3 {
            Command::new("/bin/sleep").arg("30").spawn().unwrap()
        } else {
            Command::new("/bin/true").spawn().unwrap()
        };
        if fault != 3 {
            child.wait().unwrap();
        }
        let lock = fs::metadata(path.join("owner.lock")).unwrap();
        let lease: forge_app::authorization_lease::Lease = serde_json::from_str(include_str!(
            "../../../runner/test/authorization-lease.json"
        ))
        .unwrap();
        let mut owner = lease.owner;
        owner.lock_device = lock.dev().to_string();
        owner.lock_inode = lock.ino().to_string();
        let bytes = serde_json::to_vec(&serde_json::json!({"owner": {
            "host_pid": owner.host_pid, "host_start": owner.host_start, "boot_id": owner.boot_id,
            "lock_device": owner.lock_device, "lock_inode": owner.lock_inode, "lock_fd": 3,
        }, "worker_pid": child.id(), "worker_start": owner.worker_start, "process_group": child.id()})).unwrap();
        fs::write(path.join("owner.json"), &bytes).unwrap();
        fs::set_permissions(path.join("owner.json"), fs::Permissions::from_mode(0o600)).unwrap();
        let meta = fs::metadata(&path).unwrap();
        let mut handle = Supervised {
            child: &mut child,
            path: path.clone(),
            device: meta.dev(),
            inode: meta.ino(),
            owner,
            record: serde_json::from_slice(&bytes).unwrap(),
            fenced: fault == 1,
        };
        if fault == 2 {
            fs::write(path.join("owner.json"), "{}").unwrap();
        }
        assert_eq!(handle.verify_or_exited().is_ok_and(|dead| dead), fault == 0);
        if fault == 3 {
            handle.child.kill().unwrap();
            handle.child.wait().unwrap();
        }
        if fault != 0 {
            fs::write(path.join("owner.json"), &bytes).unwrap();
            assert!(handle.verify_or_exited().is_err());
        }
        // Exited classification never permits later live signalling/verification.
        assert!(handle.verify().is_err());
        drop(handle);
        fs::remove_dir_all(path).unwrap();
    }
}
