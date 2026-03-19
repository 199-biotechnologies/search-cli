use crate::types::Mode;
use regex::Regex;
use std::sync::OnceLock;

fn news_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(latest|breaking|news|today|yesterday|this week|headlines|update|announced)\b").unwrap())
}

fn academic_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(paper|research|study|journal|pubmed|arxiv|doi|peer.?review|citation|thesis|dissertation)\b").unwrap())
}

fn scholar_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(scholar|google scholar|academic search|scientific literature)\b").unwrap())
}

fn patents_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(patent|patents|patent number|USPTO|EPO|invention|prior art)\b").unwrap())
}

fn people_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(who is|linkedin|profile|founder|ceo|cto|engineer at|works at|person)\b").unwrap())
}

fn extract_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(extract|scrape|read page|get content|full text|article text)\b").unwrap())
}

fn similar_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(similar to|like this|related to|find similar|pages like)\b").unwrap())
}

fn images_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(image|images|photo|picture|illustration|diagram)\b").unwrap())
}

fn places_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\b(near me|restaurant|hotel|directions|address|location|map|places)\b").unwrap())
}

pub fn classify_intent(query: &str) -> Mode {
    let checks: &[(Mode, &dyn Fn() -> &'static Regex)] = &[
        (Mode::News, &news_re),
        (Mode::Academic, &academic_re),
        (Mode::Scholar, &scholar_re),
        (Mode::Patents, &patents_re),
        (Mode::People, &people_re),
        (Mode::Extract, &extract_re),
        (Mode::Similar, &similar_re),
        (Mode::Images, &images_re),
        (Mode::Places, &places_re),
    ];

    for (mode, re_fn) in checks {
        if re_fn().is_match(query) {
            return *mode;
        }
    }
    Mode::General
}
