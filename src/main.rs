use std::ffi::CString;
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::ptr;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("run") => run(&args[2..]),
        Some("child") => child(&args[2..]),
        _ => panic!("Usage: mycontainer run|child <command>"),
    }
}

fn run(args: &[String]) {
    // It may be tricky initially but what it does:
    // - /proc/self/exe run /bin/bash
    // - before starting child, it calls pre_exec and clone with UTS namespace
    // - within new namespace, it runs /proc/self/exe child /bin/bash
    // - which inside sets a new hostname
    unsafe {
        Command::new("/proc/self/exe")
            .arg("child")
            .args(args)
            .pre_exec(|| {
                // Clone with UTS namespace
                libc::unshare(libc::CLONE_NEWUTS | libc::CLONE_NEWNS);
                Ok(())
            })
            .status() // spins up a new process for child() with /proc/self/exe child /bin/bash
            .expect("Failed to run child");
    }
}

fn child(args: &[String]) {
    println!("Running {:?} as PID {}", args, std::process::id());

    // This function runs inside the new UTS and mount namespaces created by run()
    // It sets up an isolated container environment with its own hostname and filesystem root
    unsafe {
        // Step 1: Set a custom hostname for this container
        // This only affects the UTS namespace, not the host system
        let hostname = b"container\0";
        let sethostname_result =
            libc::sethostname(hostname.as_ptr() as *const libc::c_char, hostname.len() - 1);

        if sethostname_result != 0 {
            eprintln!(
                "sethostname failed with: {}",
                std::io::Error::last_os_error()
            )
        } else {
            println!("Ran sethostname successfully")
        }

        // Step 2: Prepare for pivot_root by bind mounting the new root onto itself
        // This is required because pivot_root needs the new_root to be a mount point
        // MS_BIND creates a bind mount, MS_REC makes it recursive for all subdirectories
        let pivot_root_mount_result = libc::mount(
            b"/home/abc/Documents/cor/rootfs\0".as_ptr() as *const libc::c_char,
            b"/home/abc/Documents/cor/rootfs\0".as_ptr() as *const libc::c_char,
            b"\0".as_ptr() as *const libc::c_char,
            libc::MS_BIND | libc::MS_REC,
            ptr::null() as *const libc::c_void,
        );

        // Step 3: Create a directory to temporarily hold the old root filesystem
        // After pivot_root, the old root will be moved here so we can unmount it
        libc::mkdir(
            b"/home/abc/Documents/cor/rootfs/oldroot\0".as_ptr() as *const libc::c_char,
            0700,
        );

        // Step 4: Make all mounts in this namespace private
        // MS_PRIVATE prevents mount/unmount events from propagating to other namespaces
        // This ensures container filesystem changes don't affect the host
        libc::mount(
            ptr::null(),
            b"/\0".as_ptr() as *const libc::c_char,
            ptr::null(),
            libc::MS_REC | libc::MS_PRIVATE,
            ptr::null(),
        );

        // Step 5: Change to the new root directory (required for pivot_root)
        libc::chdir(b"/home/abc/Documents/cor/rootfs\0".as_ptr() as *const libc::c_char);

        // Step 6: Pivot the root filesystem
        // This swaps the root mount point: new_root becomes "/" and old "/" moves to "oldroot"
        // Arguments are relative to current directory: "." (current dir) becomes new root
        // and the old root gets moved to "./oldroot"
        let syscall_res = libc::syscall(
            libc::SYS_pivot_root,
            b".\0".as_ptr() as *const libc::c_char,
            b"oldroot\0".as_ptr() as *const libc::c_char,
        );
        if syscall_res != 0 {
            eprintln!(
                "syscall_res failed with: {}",
                std::io::Error::last_os_error()
            )
        } else {
            println!("Ran syscall_res successfully")
        }

        // Step 7: Change to the new root directory
        // After pivot_root, we're still in the old location, so move to the new "/"
        let chdir_result = libc::chdir(b"/\0".as_ptr() as *const libc::c_char);
        if chdir_result != 0 {
            eprintln!("chroot failed with: {}", std::io::Error::last_os_error())
        } else {
            println!("Ran chdir successfully")
        }

        // Step 8: Mount the /proc filesystem
        // The container needs its own /proc to see only its own processes
        // This provides process isolation from the host system
        let mount_result = libc::mount(
            b"proc\0".as_ptr() as *const libc::c_char,
            b"/proc\0".as_ptr() as *const libc::c_char,
            b"proc\0".as_ptr() as *const libc::c_char,
            0,
            ptr::null() as *const libc::c_void,
        );
        if mount_result != 0 {
            eprintln!("mount failed with: {}", std::io::Error::last_os_error())
        } else {
            println!("Ran mount successfully")
        }

        // Step 9: Unmount the old root filesystem
        // MNT_DETACH performs a lazy unmount: removes from namespace immediately
        // but cleanup happens when no longer in use
        let umount2_result = libc::umount2(
            b"/oldroot\0".as_ptr() as *const libc::c_char,
            libc::MNT_DETACH,
        );
        if umount2_result != 0 {
            eprintln!("umount2 failed with: {}", std::io::Error::last_os_error())
        } else {
            println!("Ran umount2 successfully")
        }

        // Step 10: Remove the oldroot directory now that it's unmounted
        // This cleans up the temporary mount point we created earlier
        libc::rmdir(b"/oldroot\0".as_ptr() as *const libc::c_char);
    }

    // Step 11: Execute the user's command inside the container
    // At this point, the container environment is fully set up with:
    // - Custom hostname, isolated root filesystem, and its own /proc
    Command::new(&args[0])
        .args(&args[1..])
        .status()
        .expect("Failed to run command");
}
