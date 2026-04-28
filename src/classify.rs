use crate::types::Mode;
use regex::Regex;
use std::sync::OnceLock;

fn social_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)(\btweet\b|\btweets\b|\bon twitter\b|\bon x\b|x\.com|twitter\.com|\btrending on\b|what.*\bsaying\b|@\w{1,15}\b)").unwrap())
}

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
        (Mode::Social, &social_re),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_social() {
        assert_eq!(classify_intent("tweets about rust"), Mode::Social);
        assert_eq!(classify_intent("what is @elonmusk saying"), Mode::Social);
        assert_eq!(classify_intent("trending on twitter"), Mode::Social);
    }

    #[test]
    fn test_classify_news() {
        assert_eq!(classify_intent("latest rust news"), Mode::News);
        assert_eq!(classify_intent("breaking headlines today"), Mode::News);
        assert_eq!(classify_intent("update on the situation"), Mode::News);
    }

    #[test]
    fn test_classify_academic() {
        assert_eq!(classify_intent("research paper on transformers"), Mode::Academic);
        assert_eq!(classify_intent("arxiv machine learning study"), Mode::Academic);
        assert_eq!(classify_intent("pubmed cancer journal"), Mode::Academic);
    }

    #[test]
    fn test_classify_scholar() {
        assert_eq!(classify_intent("google scholar rust"), Mode::Scholar);
        assert_eq!(classify_intent("academic search physics"), Mode::Scholar);
    }

    #[test]
    fn test_classify_patents() {
        assert_eq!(classify_intent("patent for widget"), Mode::Patents);
        assert_eq!(classify_intent("USPTO invention prior art"), Mode::Patents);
    }

    #[test]
    fn test_classify_people() {
        assert_eq!(classify_intent("who is jane doe"), Mode::People);
        assert_eq!(classify_intent("linkedin profile ceo"), Mode::People);
        assert_eq!(classify_intent("engineer at google"), Mode::People);
    }

    #[test]
    fn test_classify_extract() {
        assert_eq!(classify_intent("extract content from url"), Mode::Extract);
        assert_eq!(classify_intent("scrape this page"), Mode::Extract);
        assert_eq!(classify_intent("read page full text"), Mode::Extract);
    }

    #[test]
    fn test_classify_similar() {
        assert_eq!(classify_intent("similar to example.com"), Mode::Similar);
        assert_eq!(classify_intent("find related to rust"), Mode::Similar);
        assert_eq!(classify_intent("pages like github"), Mode::Similar);
    }

    #[test]
    fn test_classify_images() {
        assert_eq!(classify_intent("image of a cat"), Mode::Images);
        assert_eq!(classify_intent("diagram of system"), Mode::Images);
    }

    #[test]
    fn test_classify_software_engineering_queries_default_to_general() {
        // Pure programming queries with no intent keywords → General
        assert_eq!(classify_intent("rust async await tutorial"), Mode::General);
        assert_eq!(classify_intent("python list comprehension"), Mode::General);
        assert_eq!(classify_intent("how to center a div in css"), Mode::General);
        assert_eq!(classify_intent("react useEffect cleanup"), Mode::General);
        assert_eq!(classify_intent("docker compose networking"), Mode::General);
        assert_eq!(classify_intent("git rebase interactive"), Mode::General);
        assert_eq!(classify_intent("postgresql jsonb indexing"), Mode::General);
        assert_eq!(classify_intent("kubernetes pod scheduling"), Mode::General);
    }

    #[test]
    fn test_classify_se_with_intent_keywords() {
        // SE queries that contain intent-triggering words
        assert_eq!(classify_intent("latest rust release"), Mode::News);
        assert_eq!(classify_intent("research paper on large language models"), Mode::Academic);
        assert_eq!(classify_intent("arxiv transformer architecture"), Mode::Academic);
        assert_eq!(classify_intent("who is the founder of rust lang"), Mode::People);
        assert_eq!(classify_intent("linkedin profile senior engineer"), Mode::People);
        assert_eq!(classify_intent("scrape npm package readme"), Mode::Extract);
        assert_eq!(classify_intent("extract content from github readme"), Mode::Extract);
        assert_eq!(classify_intent("similar to react.dev"), Mode::Similar);
        assert_eq!(classify_intent("find similar to stackoverflow"), Mode::Similar);
        assert_eq!(classify_intent("diagram of microservices architecture"), Mode::Images);
    }

    #[test]
    fn test_classify_se_code_search_queries() {
        // Queries typical of developer code search → General
        assert_eq!(classify_intent("implement oauth2 in go"), Mode::General);
        assert_eq!(classify_intent("typescript generic constraints"), Mode::General);
        assert_eq!(classify_intent("nginx reverse proxy config"), Mode::General);
        assert_eq!(classify_intent("webpack bundle size optimization"), Mode::General);
        assert_eq!(classify_intent("redis pub/sub example"), Mode::General);
        assert_eq!(classify_intent("grpc protobuf schema definition"), Mode::General);
    }

    #[test]
    fn test_classify_places() {
        assert_eq!(classify_intent("restaurants near me"), Mode::Places);
        assert_eq!(classify_intent("hotel address location map"), Mode::Places);
    }

    #[test]
    fn test_classify_general_fallback() {
        assert_eq!(classify_intent("rust programming"), Mode::General);
        assert_eq!(classify_intent("how to bake bread"), Mode::General);
        assert_eq!(classify_intent("best laptops 2025"), Mode::General);
    }

    #[test]
    fn test_classify_priority_order() {
        // Social is checked before News; "latest tweets" matches both
        assert_eq!(classify_intent("latest tweets about rust"), Mode::Social);
        // Academic is checked before Scholar; "research paper" matches both
        assert_eq!(classify_intent("research paper on quantum computing"), Mode::Academic);
    }

    #[test]
    fn test_classify_se_framework_and_language_queries() {
        // Framework/language queries without intent keywords → General
        assert_eq!(classify_intent("nextjs app router vs pages router"), Mode::General);
        assert_eq!(classify_intent("svelte stores vs react context"), Mode::General);
        assert_eq!(classify_intent("elixir genserver pattern"), Mode::General);
        assert_eq!(classify_intent("swift concurrency async let"), Mode::General);
        assert_eq!(classify_intent("kotlin coroutines flow"), Mode::General);
        assert_eq!(classify_intent("zig allocator implementation"), Mode::General);
        assert_eq!(classify_intent("haskell monad transformer stack"), Mode::General);
        assert_eq!(classify_intent("clojure transducer composition"), Mode::General);
    }

    #[test]
    fn test_classify_se_infra_and_devops_queries() {
        assert_eq!(classify_intent("terraform module for aws vpc"), Mode::General);
        assert_eq!(classify_intent("ansible playbook best practices"), Mode::General);
        assert_eq!(classify_intent("ci cd pipeline github actions"), Mode::General);
        assert_eq!(classify_intent("prometheus grafana monitoring setup"), Mode::General);
        assert_eq!(classify_intent("istio service mesh configuration"), Mode::General);
        assert_eq!(classify_intent("aws lambda cold start optimization"), Mode::General);
    }

    #[test]
    fn test_classify_se_database_and_api_queries() {
        assert_eq!(classify_intent("mongodb aggregation pipeline"), Mode::General);
        assert_eq!(classify_intent("graphql resolver patterns"), Mode::General);
        assert_eq!(classify_intent("rest api pagination cursor vs offset"), Mode::General);
        assert_eq!(classify_intent("sql window functions example"), Mode::General);
        assert_eq!(classify_intent("prisma schema relations"), Mode::General);
        assert_eq!(classify_intent("openapi specification versioning"), Mode::General);
    }

    #[test]
    fn test_classify_se_security_and_auth_queries() {
        assert_eq!(classify_intent("jwt token validation"), Mode::General);
        assert_eq!(classify_intent("oauth2 authorization code flow"), Mode::General);
        assert_eq!(classify_intent("cors preflight request handling"), Mode::General);
        assert_eq!(classify_intent("csrf protection in express"), Mode::General);
        assert_eq!(classify_intent("bcrypt vs argon2 hashing"), Mode::General);
    }

    #[test]
    fn test_classify_se_testing_and_debugging_queries() {
        assert_eq!(classify_intent("pytest fixture scope"), Mode::General);
        assert_eq!(classify_intent("jest mock implementation"), Mode::General);
        assert_eq!(classify_intent("cypress vs playwright comparison"), Mode::General);
        assert_eq!(classify_intent("rust unit test organization"), Mode::General);
        assert_eq!(classify_intent("go race detector"), Mode::General);
    }

    #[test]
    fn test_classify_se_with_news_intent() {
        // Developer queries with news-triggering words
        assert_eq!(classify_intent("latest react 19 features"), Mode::News);
        assert_eq!(classify_intent("breaking change in node 22"), Mode::News);
        assert_eq!(classify_intent("rust 2024 edition announced"), Mode::News);
        assert_eq!(classify_intent("headlines from kubecon 2025"), Mode::News);
        assert_eq!(classify_intent("today deno 2 release update"), Mode::News);
    }

    #[test]
    fn test_classify_se_with_academic_intent() {
        // SE queries with academic keywords
        assert_eq!(classify_intent("research paper on fuzzing rust compilers"), Mode::Academic);
        assert_eq!(classify_intent("arxiv paper on neural code generation"), Mode::Academic);
        assert_eq!(classify_intent("study on developer productivity"), Mode::Academic);
        assert_eq!(classify_intent("journal of software engineering doi"), Mode::Academic);
    }

    #[test]
    fn test_classify_se_with_extract_intent() {
        // SE queries with extract/scrape keywords
        assert_eq!(classify_intent("scrape github issues"), Mode::Extract);
        assert_eq!(classify_intent("extract text from stackoverflow page"), Mode::Extract);
        assert_eq!(classify_intent("read page npmjs.com package"), Mode::Extract);
        assert_eq!(classify_intent("get content from pypi documentation"), Mode::Extract);
    }

    #[test]
    fn test_classify_se_with_people_intent() {
        // SE queries about people
        assert_eq!(classify_intent("who is the creator of linux"), Mode::People);
        assert_eq!(classify_intent("linkedin profile rust core team"), Mode::People);
        assert_eq!(classify_intent("founder of vercel"), Mode::People);
        assert_eq!(classify_intent("ceo of github"), Mode::People);
    }

    #[test]
    fn test_classify_se_with_similar_intent() {
        // SE queries looking for similar tools/pages
        assert_eq!(classify_intent("similar to tailwindcss"), Mode::Similar);
        assert_eq!(classify_intent("pages like mdn docs"), Mode::Similar);
        assert_eq!(classify_intent("related to vite build tool"), Mode::Similar);
        assert_eq!(classify_intent("find similar to supabase"), Mode::Similar);
    }

    #[test]
    fn test_classify_se_with_images_intent() {
        // SE queries about diagrams/architecture visuals
        assert_eq!(classify_intent("diagram of kubernetes architecture"), Mode::Images);
        assert_eq!(classify_intent("image of ci cd pipeline flow"), Mode::Images);
        assert_eq!(classify_intent("illustration of event loop in node"), Mode::Images);
    }

    #[test]
    fn test_classify_se_case_insensitive() {
        // Intent classification is case-insensitive
        assert_eq!(classify_intent("LATEST Python release"), Mode::News);
        assert_eq!(classify_intent("ARXIV paper transformers"), Mode::Academic);
        assert_eq!(classify_intent("SCRAPE documentation site"), Mode::Extract);
        assert_eq!(classify_intent("Similar To Figma"), Mode::Similar);
    }
}
