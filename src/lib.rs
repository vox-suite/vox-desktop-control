mod router;

#[cfg(target_os = "macos")]
mod ax;
#[cfg(target_os = "macos")]
mod click;
mod launcher;
#[cfg(target_os = "macos")]
mod ocr;

use router::Intent;

/// Result of a successfully routed GUI command.
pub enum GuiOutcome {
    Done(String),
    /// Local resolution (native command / AX tree) couldn't find a target.
    /// Carries the on-screen text candidates so a caller can hand them to an
    /// LLM as a multiple-choice pick instead of a raw screenshot.
    NeedsFallback(Vec<String>),
}

/// Runs one natural-language-ish GUI command ("open Safari", "click Sign In").
/// Never touches the network or an LLM itself — that's the caller's job when
/// this returns `NeedsFallback`.
pub fn execute(command: &str) -> Result<GuiOutcome, String> {
    match router::route(command) {
        Intent::OpenApp(name) => launcher::launch_app(&name)
            .map(|()| GuiOutcome::Done(format!("opened {name}")))
            .map_err(|e| format!("failed to open {name}: {e}")),
        Intent::ClickText(text) => click_text(&text),
        Intent::Unknown(raw) => Err(format!("no local handler for: {raw}")),
    }
}

#[cfg(target_os = "macos")]
fn click_text(text: &str) -> Result<GuiOutcome, String> {
    if !ax::is_trusted() {
        return Err(
            "needs Accessibility permission: System Settings > Privacy & Security > Accessibility"
                .to_string(),
        );
    }

    // Tier 1: accessibility tree. Cheap, exact, no Screen Recording needed.
    let elements = ax::frontmost_app_elements()?;
    if let Some(target) = ax::find_best_match(text, &elements) {
        click::click_at(target.center())?;
        return Ok(GuiOutcome::Done(format!("clicked '{}'", target.text)));
    }

    // Tier 2: local OCR over a window capture — catches custom-drawn UI the
    // AX tree doesn't expose. Needs Screen Recording permission.
    let ocr_hits = ocr::scan_frontmost_window().unwrap_or_default();
    if let Some(target) = ocr::find_best_match(text, &ocr_hits) {
        click::click_at(target.center)?;
        return Ok(GuiOutcome::Done(format!("clicked '{}'", target.text)));
    }

    // Tier 3: neither local pass found it — hand the combined candidates to
    // an LLM as a multiple-choice pick instead of a raw screenshot.
    let mut candidates: Vec<String> = elements.into_iter().map(|e| e.text).collect();
    candidates.extend(ocr_hits.into_iter().map(|h| h.text));
    Ok(GuiOutcome::NeedsFallback(candidates))
}

#[cfg(not(target_os = "macos"))]
fn click_text(_text: &str) -> Result<GuiOutcome, String> {
    Err("click is only implemented on macOS".to_string())
}
