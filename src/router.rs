pub enum Intent {
    OpenApp(String),
    ClickText(String),
    Unknown(String),
}

pub fn route(input: &str) -> Intent {
    let trimmed = input.trim();
    let lower = trimmed.to_lowercase();
    if let Some(rest) = lower.strip_prefix("open ") {
        return Intent::OpenApp(trimmed[trimmed.len() - rest.len()..].to_string());
    }
    if let Some(rest) = lower.strip_prefix("click ") {
        return Intent::ClickText(trimmed[trimmed.len() - rest.len()..].to_string());
    }
    Intent::Unknown(trimmed.to_string())
}
