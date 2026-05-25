use regex::Regex;
use std::sync::OnceLock;

pub fn four_col_table_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?m)^\|([^|]+)\|([^|]+)\|([^|]+)\|([^|]+)\|$")
            .expect("valid four-column table regex")
    })
}

pub fn three_col_table_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?m)^\|([^|]+)\|([^|]+)\|([^|]+)\|$").expect("valid three-column table regex")
    })
}
