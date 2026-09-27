pub fn launch_app(name: &str) -> Result<(), String> {
    let status = std::process::Command::new("/usr/bin/open")
        .args(["-a", name])
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("open exited with {status}"))
    }
}
