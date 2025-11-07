use std::process::Command;

/// Sets up the network namespace for the container by calling the setup script
/// Takes the PID of the child process that needs network configuration
pub fn setup_network(child_pid: u32, script_path: &str) -> Result<(), String> {
    let network_setup_result = Command::new(script_path)
        .arg(child_pid.to_string())
        .status();

    match network_setup_result {
        Ok(status) => {
            if status.success() {
                println!("Network setup completed successfully");
                Ok(())
            } else {
                let error_msg = format!("Network setup failed with exit code: {:?}", status.code());
                eprintln!("{}", error_msg);
                Err(error_msg)
            }
        }
        Err(e) => {
            let error_msg = format!("Failed to execute network setup script: {}", e);
            eprintln!("{}", error_msg);
            Err(error_msg)
        }
    }
}
