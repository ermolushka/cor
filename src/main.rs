// Import traits and types for Unix-specific process spawning
use std::os::unix::process::CommandExt;
use std::process::Command;

mod cgroups;
mod filesystem;
mod namespace;
mod network;
mod overlayfs;

use cgroups::Cgroup;
use overlayfs::OverlayFs;

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
/// It creates UTS, Mount, PID, and Network namespaces before spawning the child
fn run(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    // It may be tricky initially but what it does:
    // - /proc/self/exe run /bin/bash
    // - before starting child, it calls pre_exec and clone with UTS namespace
    // - within new namespace, it runs /proc/self/exe child /bin/bash
    // - which inside sets a new hostname

    // Generate a unique container ID for both cgroup and overlay filesystem
    let container_id = format!("mycontainer-{}", std::process::id());
    let cgroup_name = container_id.clone();

    // Set up OverlayFS before spawning the child process
    let overlay = OverlayFs::new(
        &container_id,
        "/home/abc/Documents/cor/rootfs",
        "/home/abc/Documents/cor/containers",
    );
    overlay
        .setup()
        .map_err(|e| format!("OverlayFS setup failed: {}", e))?;

    unsafe {
        // Execute /proc/self/exe (this same binary) with "child" argument
        // This creates a new process that will run the child() function
        let mut child_process = Command::new("/proc/self/exe")
            .arg("child")
            .args(args)
            .env("CGROUP_NAME", &cgroup_name)
            .env("OVERLAY_MERGED", &overlay.merged_dir)
            .pre_exec(|| namespace::create_namespaces())
            .spawn() // spawn the child process and get its PID
            .map_err(|e| format!("Failed to spawn child: {}", e))?;

        // Get the child process PID for network setup
        let child_pid = child_process.id();
        println!("Child process PID: {}", child_pid);

        // Create and configure cgroup
        // Note: We don't add child_pid here - it just waits and does nothing
        // The grandchild (forked process) will add itself to the cgroup
        let cgroup = Cgroup::new(&cgroup_name);
        let cgroups_memory_limit_result = cgroup.set_memory_limits(100 * 1024 * 1024); // 100 MB
        match cgroups_memory_limit_result {
            Ok(_) => println!("Set memory limit for cgroup {}", cgroup_name),
            Err(e) => eprintln!("Failed to set memory limit: {}", e),
        }

        let cgroup_cpu_limits_result = cgroup.set_cpu_limits(50000, 100000); // 50% CPU
        match cgroup_cpu_limits_result {
            Ok(_) => println!("Set CPU limit for cgroup {}", cgroup_name),
            Err(e) => eprintln!("Failed to set CPU limit: {}", e),
        }
        let cgroup_pid_limit_result = cgroup.set_pid_limit(&10); // max 10 processes
        match cgroup_pid_limit_result {
            Ok(_) => println!("Set PID limit for cgroup {}", cgroup_name),
            Err(e) => eprintln!("Failed to set PID limit: {}", e),
        }

        // Execute network setup script with the child PID
        let _ = network::setup_network(child_pid, "/home/abc/Documents/cor/setup-network.sh");

        // Wait for the child process to complete
        child_process
            .wait()
            .map_err(|e| format!("Failed to wait for child: {}", e))?;

        // Clean up the OverlayFS after container exits
        overlay
            .teardown()
            .map_err(|e| format!("OverlayFS cleanup failed: {}", e))?;
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
    let pid = namespace::fork_into_pid_namespace()?;

    if pid > 0 {
        // Parent: wait for child to complete all container setup and execution
        let exit_code = namespace::wait_for_child(pid);
        std::process::exit(exit_code);
    }

    // Child continues here - NOW we're truly PID 1 in the new namespace!
    println!("After fork, PID is now: {}", std::process::id());

    // Add this process (the actual worker) to the cgroup
    let child_pid = std::process::id();
    let cgroup_name =
        std::env::var("CGROUP_NAME").expect("CGROUP_NAME environment variable not set");

    // Reuse the existing cgroup
    let cgroup = Cgroup::new(&cgroup_name);
    cgroup
        .add_process_cgroup(child_pid)
        .expect("Failed to add process to cgroup");
    println!("Added PID {} to cgroup {}", child_pid, cgroup_name);

    // Important: Don't let the Cgroup drop here, as it would delete the cgroup directory
    // The parent process owns the cgroup lifecycle
    std::mem::forget(cgroup);

    // Set up the container environment: hostname and filesystem isolation
    namespace::set_hostname("container")?;

    // Get the overlay merged directory from environment variable
    let merged_dir =
        std::env::var("OVERLAY_MERGED").expect("OVERLAY_MERGED environment variable not set");

    filesystem::setup_container_filesystem(&merged_dir)
        .map_err(|e| format!("Filesystem setup failed: {}", e))?;

    // Install zombie reaper before running user command
    // This ensures PID 1 automatically reaps orphaned processes
    namespace::setup_zombie_reaper()
        .map_err(|e| format!("Failed to setup zombie reaper: {}", e))?;

    // Step 12: Execute the user's command inside the container
    // At this point, the container environment is fully set up with:
    // - Custom hostname, isolated root filesystem, its own /proc, and DNS configuration
    Command::new(&args[0])
        .args(&args[1..])
        .status()
        .map_err(|e| format!("Failed to run command: {}", e))?;

    Ok(())
}
