// Import traits and types for Unix-specific process spawning
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::ptr;

/// Helper function to check libc call results and convert to Result
/// Takes a libc return value (0 = success, non-zero = error) and operation name
/// Returns Ok(()) on success, Err with last OS error on failure
fn check_libc_result(result: i32, operation: &str) -> Result<(), std::io::Error> {
    if result != 0 {
        let err = std::io::Error::last_os_error();
        eprintln!("{} failed: {}", operation, err);
        Err(err)
    } else {
        println!("{} successful", operation);
        Ok(())
    }
}

/// Main entry point for the container runtime
/// Parses command-line arguments and dispatches to run() or child() functions
fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Dispatch to appropriate handler based on first argument
    let result = match args.get(1).map(|s| s.as_str()) {
        Some("run") => run(&args[2..]),
        Some("child") => child(&args[2..]),
        _ => {
            eprintln!("Usage: mycontainer run|child <command>");
            std::process::exit(1);
        }
    };

    // Handle any errors from run() or child() by printing and exiting
    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}

/// The "run" function - creates new namespaces and spawns the child process
/// This is called when user runs: mycontainer run <command>
/// It creates UTS, Mount, and PID namespaces before spawning the child
fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    // It may be tricky initially but what it does:
    // - /proc/self/exe run /bin/bash
    // - before starting child, it calls pre_exec and clone with UTS namespace
    // - within new namespace, it runs /proc/self/exe child /bin/bash
    // - which inside sets a new hostname
    unsafe {
        // Execute /proc/self/exe (this same binary) with "child" argument
        // This creates a new process that will run the child() function
        Command::new("/proc/self/exe")
            .arg("child")
            .args(args)
            .pre_exec(|| {
                // Clone with UTS namespace for hostname isolation
                // CLONE_NEWNS for mount namespace (filesystem isolation)
                // CLONE_NEWPID for PID namespace (process isolation)
                let result =
                    libc::unshare(libc::CLONE_NEWUTS | libc::CLONE_NEWNS | libc::CLONE_NEWPID);
                if result != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            })
            .status() // spins up a new process for child() with /proc/self/exe child /bin/bash
            .map_err(|e| format!("Failed to run child: {}", e))?;
    }

    Ok(())
}

/// The "child" function - sets up the isolated container environment
/// This runs inside the new namespaces created by run()
/// It performs all container setup: hostname, filesystem pivot, /proc mount, etc.
fn child(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    println!("Running {:?} as PID {}", args, std::process::id());

    //  This process gets PID 1 in the new namespace, but it's already running the child() function.
    // If it tries to do the container setup (pivot_root, mount /proc, etc.), it might be too late or in
    // the wrong state

    // Fork one more time to truly enter the PID namespace
    // The process that calls unshare(CLONE_NEWPID) doesn't enter the namespace itself
    // Only its children do. So we need to fork again and let the child do the setup.
    // The process that calls unshare(CLONE_NEWPID) has already initialized its process
    // state before creating the namespace. It needs a "fresh start" process that's
    // born inside the new namespace to properly be PID 1.

    // New process will have PID 2. When the parent (PID 1) exits, the child effectively
    // becomes the namespace's init process with responsibilities of PID 1

    // Fork to create a process that is truly born inside the PID namespace
    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            // Fork failed - return the error
            return Err(std::io::Error::last_os_error().into());
        }

        // What's happening:
        // 1. Parent (child() PID 1) waits for child (PID 2) to complete all container setup
        // 2. WIFEXITED(status) checks if child exited normally
        // 3. WEXITSTATUS(status) extracts the exit code
        // 4. Parent exits with same code as child (propagates errors)
        if pid > 0 {
            // Parent: wait for child to complete all container setup and execution
            let mut status: i32 = 0;
            libc::waitpid(pid, &mut status, 0); // Block until child exits

            // Exit with the same status as the child to propagate exit codes
            std::process::exit(if libc::WIFEXITED(status) {
                libc::WEXITSTATUS(status) // Normal exit - use child's exit code
            } else {
                1 // Abnormal exit (signal, etc.) - exit with error
            });
        }

        // Child continues here - NOW we're truly PID 1 in the new namespace!
        // This forked child process can now properly set up the container environment
        println!("After fork, PID is now: {}", std::process::id());
    }

    // This function runs inside the new UTS and mount namespaces created by run()
    // It sets up an isolated container environment with its own hostname and filesystem root
    unsafe {
        // Step 1: Make all mounts in this namespace private FIRST
        // MS_PRIVATE prevents mount/unmount events from propagating to other namespaces
        // This MUST be done before any other mounts to prevent leaking to the host
        // MS_REC applies recursively to all mount points in the namespace
        let result = libc::mount(
            ptr::null(),
            b"/\0".as_ptr() as *const libc::c_char,
            ptr::null(),
            libc::MS_REC | libc::MS_PRIVATE,
            ptr::null(),
        );
        check_libc_result(result, "mount (private)")?;

        // Step 2: Set a custom hostname for this container
        // This only affects the UTS namespace, not the host system
        let hostname = b"container\0";
        let result =
            libc::sethostname(hostname.as_ptr() as *const libc::c_char, hostname.len() - 1);
        check_libc_result(result, "sethostname")?;

        // Step 3: Prepare for pivot_root by bind mounting the new root onto itself
        // This is required because pivot_root needs the new_root to be a mount point
        // MS_BIND creates a bind mount, MS_REC makes it recursive for all subdirectories
        // Now that we've set MS_PRIVATE, this mount won't propagate to the host
        let result = libc::mount(
            b"/home/abc/Documents/cor/rootfs\0".as_ptr() as *const libc::c_char,
            b"/home/abc/Documents/cor/rootfs\0".as_ptr() as *const libc::c_char,
            b"\0".as_ptr() as *const libc::c_char,
            libc::MS_BIND | libc::MS_REC,
            ptr::null() as *const libc::c_void,
        );
        check_libc_result(result, "mount (bind rootfs)")?;

        // Step 4: Create a directory to temporarily hold the old root filesystem
        // After pivot_root, the old root will be moved here so we can unmount it
        let result = libc::mkdir(
            b"/home/abc/Documents/cor/rootfs/oldroot\0".as_ptr() as *const libc::c_char,
            0o700,
        );
        check_libc_result(result, "mkdir (oldroot)")?;

        // Step 5: Change to the new root directory (required for pivot_root)
        let result =
            libc::chdir(b"/home/abc/Documents/cor/rootfs\0".as_ptr() as *const libc::c_char);
        check_libc_result(result, "chdir (to rootfs)")?;

        // Step 6: Pivot the root filesystem
        // This swaps the root mount point: new_root becomes "/" and old "/" moves to "oldroot"
        // Arguments are relative to current directory: "." (current dir) becomes new root
        // and the old root gets moved to "./oldroot"
        let result = libc::syscall(
            libc::SYS_pivot_root,
            b".\0".as_ptr() as *const libc::c_char,
            b"oldroot\0".as_ptr() as *const libc::c_char,
        ) as i32;
        check_libc_result(result, "pivot_root")?;

        // Step 7: Change to the new root directory
        // After pivot_root, we're still in the old location, so move to the new "/"
        let result = libc::chdir(b"/\0".as_ptr() as *const libc::c_char);
        check_libc_result(result, "chdir (to /)")?;

        // Step 8: Mount the /proc filesystem
        // The container needs its own /proc to see only its own processes
        // This provides process isolation from the host system
        let result = libc::mount(
            b"proc\0".as_ptr() as *const libc::c_char,
            b"/proc\0".as_ptr() as *const libc::c_char,
            b"proc\0".as_ptr() as *const libc::c_char,
            0,
            ptr::null() as *const libc::c_void,
        );
        check_libc_result(result, "mount (proc)")?;

        // Step 9: Unmount the old root filesystem
        // MNT_DETACH performs a lazy unmount: removes from namespace immediately
        // but cleanup happens when no longer in use
        let result = libc::umount2(
            b"/oldroot\0".as_ptr() as *const libc::c_char,
            libc::MNT_DETACH,
        );
        check_libc_result(result, "umount2 (oldroot)")?;

        // Step 10: Remove the oldroot directory now that it's unmounted
        // This cleans up the temporary mount point we created earlier
        let result = libc::rmdir(b"/oldroot\0".as_ptr() as *const libc::c_char);
        check_libc_result(result, "rmdir (oldroot)")?;
    }

    // Step 11: Execute the user's command inside the container
    // At this point, the container environment is fully set up with:
    // - Custom hostname, isolated root filesystem, and its own /proc
    Command::new(&args[0])
        .args(&args[1..])
        .status()
        .map_err(|e| format!("Failed to run command: {}", e))?;

    Ok(())
}
