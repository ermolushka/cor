use std::os::unix::process::CommandExt;
use std::process::Command;

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
                libc::unshare(libc::CLONE_NEWUTS);
                Ok(())
            })
            .status() // spins up a new process for child() with /proc/self/exe child /bin/bash
            .expect("Failed to run child");
    }
}

fn child(args: &[String]) {
    println!("Running {:?} as PID {}", args, std::process::id());

    // Set hostname
    unsafe {
        let hostname = b"container\0";
        libc::sethostname(hostname.as_ptr() as *const libc::c_char, hostname.len() - 1);
    }

    // Execute the command
    Command::new(&args[0])
        .args(&args[1..])
        .status()
        .expect("Failed to run command");
}
