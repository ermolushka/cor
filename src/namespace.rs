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
