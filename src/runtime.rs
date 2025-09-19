// Minimal validators and helpers for SCORM 1.2

pub fn is_valid_element_12(el: &str) -> bool {
    matches!(
        el,
        "cmi.core.lesson_status"
            | "cmi.core.lesson_location"
            | "cmi.core.score.raw"
            | "cmi.suspend_data"
            | "cmi.core.session_time"
            | "cmi.core.exit"
    )
}

pub fn max_len(el: &str) -> usize {
    match el {
        "cmi.suspend_data" => 4096, // common de facto 1.2 limit
        _ => 255,
    }
}

pub fn normalize_lesson_status(v: &str) -> Option<&'static str> {
    match v {
        "passed"        => Some("passed"),
        "failed"        => Some("failed"),
        "completed"     => Some("completed"),
        "incomplete"    => Some("incomplete"),
        "browsed"       => Some("browsed"),
        "not attempted" => Some("not attempted"),
        _ => None,
    }
}

