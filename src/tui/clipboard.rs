//! Clipboard utilities for copying text to system clipboard.

use anyhow::{anyhow, Result};
use std::process::Command;

/// Copy text to system clipboard
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    // Try different clipboard tools based on platform
    #[cfg(target_os = "macos")]
    {
        copy_with_pbcopy(text)
    }

    #[cfg(target_os = "linux")]
    {
        // Try wl-copy first (Wayland), then xclip (X11)
        copy_with_wl_copy(text)
            .or_else(|_| copy_with_xclip(text))
            .or_else(|_| copy_with_xsel(text))
    }

    #[cfg(target_os = "windows")]
    {
        copy_with_powershell(text)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(anyhow!(
            "Clipboard operations not supported on this platform"
        ))
    }
}

#[cfg(target_os = "macos")]
fn copy_with_pbcopy(text: &str) -> Result<()> {
    let mut child = Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }

    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("pbcopy failed"))
    }
}

#[cfg(target_os = "linux")]
fn copy_with_wl_copy(text: &str) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }

    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("wl-copy failed"))
    }
}

#[cfg(target_os = "linux")]
fn copy_with_xclip(text: &str) -> Result<()> {
    let mut child = Command::new("xclip")
        .args(["-selection", "clipboard"])
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }

    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("xclip failed"))
    }
}

#[cfg(target_os = "linux")]
fn copy_with_xsel(text: &str) -> Result<()> {
    let mut child = Command::new("xsel")
        .args(["--clipboard", "--input"])
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }

    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("xsel failed"))
    }
}

#[cfg(target_os = "windows")]
fn copy_with_powershell(text: &str) -> Result<()> {
    let mut child = Command::new("powershell")
        .args(["-Command", "Set-Clipboard"])
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }

    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("powershell Set-Clipboard failed"))
    }
}
