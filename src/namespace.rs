use std::io;

/// Creates new namespaces for container isolation
/// This includes UTS (hostname), Mount (filesystem), PID (process), and Network namespaces
pub fn create_namespaces() -> io::Result<()> {
    unsafe {
        let result = libc::unshare(
            libc::CLONE_NEWUTS | libc::CLONE_NEWNS | libc::CLONE_NEWPID | libc::CLONE_NEWNET,
        );
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Sets the hostname for the current UTS namespace
/// This only affects the container, not the host system
pub fn set_hostname(hostname: &str) -> io::Result<()> {
    let hostname_cstr = format!("{}\0", hostname);
    unsafe {
        let result = libc::sethostname(
            hostname_cstr.as_ptr() as *const libc::c_char,
            hostname.len(),
        );
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    println!("sethostname successful");
    Ok(())
}

/// Forks the current process to fully enter the PID namespace
/// Returns the child PID to the parent, or 0 to the child
/// The process that calls unshare(CLONE_NEWPID) doesn't enter the namespace itself,
/// only its children do. This function creates that child process.
pub fn fork_into_pid_namespace() -> io::Result<i32> {
    unsafe {
        let pid = libc::fork();
        if pid < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(pid)
    }
}

/// Waits for a child process and returns its exit status
/// Used by the parent process after forking into the PID namespace
pub fn wait_for_child(pid: i32) -> i32 {
    unsafe {
        let mut status: i32 = 0;
        libc::waitpid(pid, &mut status, 0);

        if libc::WIFEXITED(status) {
            libc::WEXITSTATUS(status)
        } else {
            1 // Abnormal exit
        }
    }
}

/// Signal handler for SIGCHLD that reaps zombie processes
/// This is called automatically when a child process dies
/// It uses waitpid with WNOHANG to collect all dead children without blocking
extern "C" fn sigchld_handler(_signum: i32) {
    unsafe {
        // Loop to reap all zombie children
        // waitpid with pid=-1 waits for any child
        // WNOHANG makes it non-blocking, returning 0 if no zombies exist
        loop {
            let pid = libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG);
            if pid <= 0 {
                // No more zombies to reap (pid==0) or error (pid==-1)
                break;
            }
        }
    }
}

/// Installs a SIGCHLD signal handler to automatically reap zombie processes
/// This is essential for PID 1 in a PID namespace, which inherits all orphaned processes
/// Must be called before exec'ing the user's command
pub fn setup_zombie_reaper() -> io::Result<()> {
    unsafe {
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigchld_handler as usize;
        sa.sa_flags = libc::SA_RESTART | libc::SA_NOCLDSTOP;

        // Initialize the signal mask to empty (no signals blocked)
        libc::sigemptyset(&mut sa.sa_mask);

        // Install the signal handler
        let result = libc::sigaction(libc::SIGCHLD, &sa, std::ptr::null_mut());
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    println!("Zombie reaper installed for PID {}", std::process::id());
    Ok(())
}
