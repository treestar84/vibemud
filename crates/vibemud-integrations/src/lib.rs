pub fn tmux_status_binding() -> &'static str {
    "bind-key g display-menu \\\n  \"VibeMUD Hunt\" a \"run-shell 'mudctl a'\" \\\n  \"VibeMUD Character\" c \"run-shell 'mudctl c'\" \\\n  \"VibeMUD Close\" x \"run-shell 'mudctl x'\" \\\n  \"VibeMUD Map\" m \"run-shell 'mudctl m'\" \\\n  \"VibeMUD Stop\" s \"run-shell 'mudctl s'\""
}

pub fn adapter_privacy_notice() -> &'static str {
    "When enabled, /mud commands call mudctl and may place game status output into the Claude/Codex conversation context. VibeMUD never reads code, prompts, file contents, commit messages, or absolute source paths for gameplay."
}

pub fn codex_adapter_template() -> &'static str {
    "# VibeMUD /mud adapter\nUse: /mud status -> run `mudctl status`. Privacy: game output can enter chat context; code/prompt/file content is never read."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_notice_mentions_context() {
        assert!(adapter_privacy_notice().contains("conversation context"));
    }
}
