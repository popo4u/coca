use std::fmt;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderKind {
    Codex,
    Claude,
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderKind::Codex => write!(f, "codex"),
            ProviderKind::Claude => write!(f, "claude"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderFilter {
    All,
    Codex,
    Claude,
}

impl ProviderFilter {
    pub fn includes(self, provider: ProviderKind) -> bool {
        matches!(
            (self, provider),
            (ProviderFilter::All, _)
                | (ProviderFilter::Codex, ProviderKind::Codex)
                | (ProviderFilter::Claude, ProviderKind::Claude)
        )
    }

    pub fn next(self) -> Self {
        match self {
            ProviderFilter::All => ProviderFilter::Codex,
            ProviderFilter::Codex => ProviderFilter::Claude,
            ProviderFilter::Claude => ProviderFilter::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ProviderFilter::All => "all",
            ProviderFilter::Codex => "codex",
            ProviderFilter::Claude => "claude",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub text: String,
    pub timestamp_ms: Option<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Session {
    pub provider: ProviderKind,
    pub id: String,
    pub title: String,
    pub cwd: String,
    pub created_at_ms: Option<i64>,
    pub updated_at_ms: Option<i64>,
    pub model: Option<String>,
    pub source_path: PathBuf,
    pub first_user_message: Option<String>,
    pub transcript: Vec<ChatMessage>,
    pub resume_program: String,
    pub resume_args: Vec<String>,
}

impl Session {
    pub fn searchable_text(&self) -> String {
        let mut parts = vec![
            self.provider.to_string(),
            self.id.clone(),
            self.title.clone(),
            self.cwd.clone(),
        ];
        if let Some(model) = &self.model {
            parts.push(model.clone());
        }
        if let Some(first) = &self.first_user_message {
            parts.push(first.clone());
        }
        for message in &self.transcript {
            parts.push(message.role.clone());
            parts.push(message.text.clone());
        }
        parts.join("\n").to_lowercase()
    }
}

pub(crate) fn truncate_for_title(text: &str, max_chars: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for (idx, ch) in collapsed.chars().enumerate() {
        if idx >= max_chars {
            out.push('…');
            return out;
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_filter_cycles() {
        assert_eq!(ProviderFilter::All.next(), ProviderFilter::Codex);
        assert_eq!(ProviderFilter::Codex.next(), ProviderFilter::Claude);
        assert_eq!(ProviderFilter::Claude.next(), ProviderFilter::All);
    }

    #[test]
    fn truncates_titles() {
        assert_eq!(truncate_for_title("hello\n  world", 20), "hello world");
        assert_eq!(truncate_for_title("abcdef", 3), "abc…");
    }
}
