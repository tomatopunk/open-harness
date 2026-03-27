use std::path::{Component, Path, PathBuf};

pub fn sanitize_path_component(input: &str) -> Option<String> {
    let path = Path::new(input);
    if path.components().count() != 1 {
        return None;
    }
    match path.components().next()? {
        Component::Normal(name) => {
            let candidate = name.to_string_lossy().to_string();
            if candidate.is_empty() || candidate.contains(std::path::MAIN_SEPARATOR) {
                None
            } else {
                Some(candidate)
            }
        }
        _ => None,
    }
}

pub fn sanitize_relative_path(input: &str) -> Option<PathBuf> {
    let path = Path::new(input);
    if path.is_absolute() {
        return None;
    }
    let mut clean = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Normal(seg) => clean.push(seg),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if clean.as_os_str().is_empty() {
        None
    } else {
        Some(clean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_rejects_traversal() {
        assert!(sanitize_path_component("../passwd").is_none());
        assert!(sanitize_path_component("a/b.txt").is_none());
        assert_eq!(sanitize_path_component("safe.txt"), Some("safe.txt".to_string()));
    }

    #[test]
    fn relative_path_rejects_parent_dir() {
        assert!(sanitize_relative_path("../a").is_none());
        assert!(sanitize_relative_path("/tmp/a").is_none());
        assert_eq!(sanitize_relative_path("foo/bar.txt"), Some(PathBuf::from("foo/bar.txt")));
    }
}
