pub struct Cgroup {
    pub name: String,
    pub path: String,
}

impl Cgroup {
    pub fn new(name: &str) -> Self {
        let cgroup_path = format!("/sys/fs/cgroup/{}", name);
        // Only create the directory if it doesn't exist
        // This allows reusing the same Cgroup from different processes
        if !std::path::Path::new(&cgroup_path).exists() {
            std::fs::create_dir_all(&cgroup_path).expect("failed to create cgroup path");
        }
        Cgroup {
            name: name.to_string(),
            path: cgroup_path,
        }
    }

    pub fn set_memory_limits(&self, limit_in_bytes: i32) -> Result<(), std::io::Error> {
        let memory_limit_path = format!("{}/memory.max", self.path);
        std::fs::write(memory_limit_path, limit_in_bytes.to_string())?;
        Ok(())
    }

    pub fn set_cpu_limits(&self, quota: i32, period: i32) -> Result<(), std::io::Error> {
        // quota: microseconds of CPU time per period
        // period: microseconds
        // Example: quota=50000, period=100000 = 50% of one CPU core

        let cpu_limit_path = format!("{}/cpu.max", self.path);
        let cpu_content = format!("{} {}", quota, period);
        std::fs::write(cpu_limit_path, cpu_content)?;
        Ok(())
    }

    pub fn set_pid_limit(&self, pid: &u32) -> Result<(), std::io::Error> {
        // max num of processes in cgroup
        let memory_limit_path = format!("{}/pids.max", self.path);
        std::fs::write(memory_limit_path, pid.to_string())?;
        Ok(())
    }

    pub fn add_process_cgroup(&self, pid: u32) -> Result<(), std::io::Error> {
        let cgroup_procs_path = format!("{}/cgroup.procs", self.path);
        std::fs::write(cgroup_procs_path, pid.to_string())?;
        Ok(())
    }
}

impl Drop for Cgroup {
    fn drop(&mut self) {
        // First, try to kill any remaining processes in the cgroup
        let cgroup_procs_path = format!("{}/cgroup.procs", self.path);

        // Read all PIDs still in the cgroup and kill them
        if let Ok(procs_content) = std::fs::read_to_string(&cgroup_procs_path) {
            for line in procs_content.lines() {
                if let Ok(pid) = line.trim().parse::<i32>() {
                    unsafe {
                        // Send SIGKILL to any remaining processes
                        libc::kill(pid, libc::SIGKILL);
                    }
                }
            }
        }

        // Give the kernel a moment to clean up
        std::thread::sleep(std::time::Duration::from_millis(100));

        // Now try to remove the cgroup directory
        if let Err(e) = std::fs::remove_dir(&self.path) {
            eprintln!(
                "Warning: Failed to remove cgroup directory {}: {}",
                self.path, e
            );
        } else {
            println!("Cleaned up cgroup: {}", self.name);
        }
    }
}
