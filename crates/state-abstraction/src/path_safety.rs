pub fn sanitize_thread_id(input: &str) -> Option<String> {
    if input.is_empty() {
        return None;
    }
    if input.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') {
        Some(input.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_thread_id_allows_safe_values() {
        assert_eq!(sanitize_thread_id("thread_1-abc"), Some("thread_1-abc".to_string()));
    }

    #[test]
    fn sanitize_thread_id_rejects_unsafe_values() {
        assert!(sanitize_thread_id("").is_none());
        assert!(sanitize_thread_id("../a").is_none());
        assert!(sanitize_thread_id("a/b").is_none());
        assert!(sanitize_thread_id("a.b").is_none());
    }
}
