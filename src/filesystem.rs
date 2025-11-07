use std::io;
use std::ptr;

/// Helper function to check libc call results and convert to Result
fn check_libc_result(result: i32, operation: &str) -> io::Result<()> {
    if result != 0 {
        let err = io::Error::last_os_error();
        eprintln!("{} failed: {}", operation, err);
        Err(err)
    } else {
        println!("{} successful", operation);
        Ok(())
    }
}

/// Makes all mounts in the current namespace private
/// This prevents mount/unmount events from propagating to other namespaces
/// Must be called before any other mounts to prevent leaking to the host
pub fn make_mounts_private() -> io::Result<()> {
    unsafe {
        let result = libc::mount(
            ptr::null(),
            b"/\0".as_ptr() as *const libc::c_char,
            ptr::null(),
            libc::MS_REC | libc::MS_PRIVATE,
            ptr::null(),
        );
        check_libc_result(result, "mount (private)")
    }
}

/// Binds the rootfs directory to itself to prepare for pivot_root
/// pivot_root requires the new_root to be a mount point
pub fn bind_mount_rootfs(rootfs_path: &str) -> io::Result<()> {
    let rootfs_cstr = format!("{}\0", rootfs_path);
    unsafe {
        let result = libc::mount(
            rootfs_cstr.as_ptr() as *const libc::c_char,
            rootfs_cstr.as_ptr() as *const libc::c_char,
            b"\0".as_ptr() as *const libc::c_char,
            libc::MS_BIND | libc::MS_REC,
            ptr::null() as *const libc::c_void,
        );
        check_libc_result(result, "mount (bind rootfs)")
    }
}

/// Creates the oldroot directory to temporarily hold the old root filesystem
pub fn create_oldroot_dir(rootfs_path: &str) -> io::Result<()> {
    let oldroot_path = format!("{}/oldroot\0", rootfs_path);
    unsafe {
        let result = libc::mkdir(oldroot_path.as_ptr() as *const libc::c_char, 0o700);
        check_libc_result(result, "mkdir (oldroot)")
    }
}

/// Pivots the root filesystem to the new rootfs
/// After this, the new rootfs becomes "/" and the old root is moved to oldroot
pub fn pivot_root(rootfs_path: &str) -> io::Result<()> {
    let rootfs_cstr = format!("{}\0", rootfs_path);

    // Change to the new root directory
    unsafe {
        let result = libc::chdir(rootfs_cstr.as_ptr() as *const libc::c_char);
        check_libc_result(result, "chdir (to rootfs)")?;

        // Pivot the root filesystem
        let result = libc::syscall(
            libc::SYS_pivot_root,
            b".\0".as_ptr() as *const libc::c_char,
            b"oldroot\0".as_ptr() as *const libc::c_char,
        ) as i32;
        check_libc_result(result, "pivot_root")?;

        // Change to the new root directory
        let result = libc::chdir(b"/\0".as_ptr() as *const libc::c_char);
        check_libc_result(result, "chdir (to /)")
    }
}

/// Mounts the /proc filesystem for the container
/// This provides process isolation from the host system
pub fn mount_proc() -> io::Result<()> {
    unsafe {
        let result = libc::mount(
            b"proc\0".as_ptr() as *const libc::c_char,
            b"/proc\0".as_ptr() as *const libc::c_char,
            b"proc\0".as_ptr() as *const libc::c_char,
            0,
            ptr::null() as *const libc::c_void,
        );
        check_libc_result(result, "mount (proc)")
    }
}

/// Unmounts and removes the old root filesystem
/// This completes the filesystem isolation setup
pub fn cleanup_oldroot() -> io::Result<()> {
    unsafe {
        // Lazy unmount the old root
        let result = libc::umount2(
            b"/oldroot\0".as_ptr() as *const libc::c_char,
            libc::MNT_DETACH,
        );
        check_libc_result(result, "umount2 (oldroot)")?;

        // Remove the oldroot directory
        let result = libc::rmdir(b"/oldroot\0".as_ptr() as *const libc::c_char);
        check_libc_result(result, "rmdir (oldroot)")
    }
}

/// Configures DNS resolution by writing to /etc/resolv.conf
pub fn setup_dns() -> io::Result<()> {
    let dns_config = "nameserver 8.8.8.8\nnameserver 8.8.4.4\n";
    std::fs::write("/etc/resolv.conf", dns_config)?;
    println!("DNS configuration written to /etc/resolv.conf");
    Ok(())
}

/// Performs the complete filesystem setup for the container
/// This includes all mount operations, pivot_root, and DNS configuration
pub fn setup_container_filesystem(rootfs_path: &str) -> io::Result<()> {
    make_mounts_private()?;
    bind_mount_rootfs(rootfs_path)?;
    create_oldroot_dir(rootfs_path)?;
    pivot_root(rootfs_path)?;
    mount_proc()?;
    cleanup_oldroot()?;
    setup_dns()?;
    Ok(())
}
