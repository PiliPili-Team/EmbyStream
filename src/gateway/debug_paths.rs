use once_cell::sync::Lazy;
use regex::Regex;

/// Path patterns for high-volume Emby API and Web UI traffic.
/// These paths will use DEBUG level in LoggerMiddleware to reduce log noise.
///
/// ## How to add a new pattern
///
/// Add a regex string to match paths that should be logged at DEBUG.
/// Typically: /emby/* API and /web/* static assets.
const DEBUG_PATH_PATTERNS: &[&str] = &[r"(?i)^/(?:emby/|web/)"];

static COMPILED_DEBUG_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    DEBUG_PATH_PATTERNS
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect()
});

/// Returns true if the path is a high-volume API path (Emby API or Web UI).
/// These should use DEBUG log level.
pub fn is_debug_path(path: &str) -> bool {
    COMPILED_DEBUG_PATTERNS.iter().any(|re| re.is_match(path))
}
