use std::fs;
use std::io;
use std::path::Path;

/// Represents the directory structure for an OverlayFS mount
/// Each container gets its own set of directories for isolation
pub struct OverlayFs {
    pub container_id: String,
    pub lower_dir: String,  // Read-only base filesystem (shared)
    pub upper_dir: String,  // Container's writable layer
    pub work_dir: String, // OverlayFS working directory (for atomic operations and temp file handling from lower to upper, for example)
    pub merged_dir: String, // Final merged view (mount point)
}

impl OverlayFs {
    /// Creates a new OverlayFS configuration for a container
    ///
    /// # Arguments
    /// * `container_id` - Unique identifier for this container
    /// * `rootfs_path` - Path to the base filesystem (lower layer)
    /// * `containers_base` - Base directory for all container data
    pub fn new(container_id: &str, rootfs_path: &str, containers_base: &str) -> Self {
        let container_dir = format!("{}/{}", containers_base, container_id);

        Self {
            container_id: container_id.to_string(),
            lower_dir: rootfs_path.to_string(),
            upper_dir: format!("{}/upper", container_dir),
            work_dir: format!("{}/work", container_dir),
            merged_dir: format!("{}/merged", container_dir),
        }
    }

    /// Creates the directory structure needed for OverlayFS
    /// Creates upper, work, and merged directories for this container
    pub fn create_directories(&self) -> io::Result<()> {
        println!(
            "Creating overlay directories for container {}",
            self.container_id
        );

        fs::create_dir_all(&self.upper_dir)?;
        fs::create_dir_all(&self.work_dir)?;
        fs::create_dir_all(&self.merged_dir)?;

        println!("  Upper: {}", self.upper_dir);
        println!("  Work:  {}", self.work_dir);
        println!("  Merged: {}", self.merged_dir);

        Ok(())
    }

    /// Mounts the OverlayFS with the configured directories
    /// This creates the unified view combining lower (read-only) and upper (writable) layers
    pub fn mount(&self) -> io::Result<()> {
        // Build the options string: lowerdir=<path>,upperdir=<path>,workdir=<path>
        let options = format!(
            "lowerdir={},upperdir={},workdir={}\0",
            self.lower_dir, self.upper_dir, self.work_dir
        );

        let merged_cstr = format!("{}\0", self.merged_dir);

        println!("Mounting OverlayFS:");
        println!("  Lower (RO): {}", self.lower_dir);
        println!("  Upper (RW): {}", self.upper_dir);
        println!("  Merged:     {}", self.merged_dir);

        unsafe {
            let result = libc::mount(
                b"overlay\0".as_ptr() as *const libc::c_char,
                merged_cstr.as_ptr() as *const libc::c_char,
                b"overlay\0".as_ptr() as *const libc::c_char,
                0,
                options.as_ptr() as *const libc::c_void,
            );

            if result != 0 {
                let err = io::Error::last_os_error();
                eprintln!("OverlayFS mount failed: {}", err);
                Err(err)
            } else {
                println!("OverlayFS mounted successfully");
                Ok(())
            }
        }
    }

    /// Unmounts the OverlayFS merged directory
    pub fn unmount(&self) -> io::Result<()> {
        let merged_cstr = format!("{}\0", self.merged_dir);

        println!("Unmounting OverlayFS at {}", self.merged_dir);

        unsafe {
            let result = libc::umount2(
                merged_cstr.as_ptr() as *const libc::c_char,
                libc::MNT_DETACH, // Lazy unmount
            );

            if result != 0 {
                let err = io::Error::last_os_error();
                eprintln!("OverlayFS unmount failed: {}", err);
                Err(err)
            } else {
                println!("OverlayFS unmounted successfully");
                Ok(())
            }
        }
    }

    /// Removes all container directories after unmounting
    /// This cleans up the upper, work, and merged directories
    pub fn cleanup(&self) -> io::Result<()> {
        println!(
            "Cleaning up container directories for {}",
            self.container_id
        );

        // Remove the entire container directory
        if let Some(parent) = Path::new(&self.upper_dir).parent() {
            if parent.exists() {
                fs::remove_dir_all(parent)?;
                println!("Removed container directory: {}", parent.display());
            }
        }

        Ok(())
    }

    /// Complete setup: creates directories and mounts OverlayFS
    pub fn setup(&self) -> io::Result<()> {
        self.create_directories()?;
        self.mount()?;
        Ok(())
    }

    /// Complete teardown: unmounts and removes directories
    pub fn teardown(&self) -> io::Result<()> {
        self.unmount()?;
        self.cleanup()?;
        Ok(())
    }
}
