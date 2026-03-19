use crate::errors::SearchError;
use crate::types::SearchResponse;

pub fn render(response: &SearchResponse) {
    let json = serde_json::to_string_pretty(response).expect("failed to serialize response");
    println!("{json}");
}

pub fn render_error(error: &SearchError) {
    let err_response = error.to_error_response();
    let json = serde_json::to_string_pretty(&err_response).expect("failed to serialize error");
    eprintln!("{json}");
}

pub fn render_value(value: &serde_json::Value) {
    let json = serde_json::to_string_pretty(value).expect("failed to serialize value");
    println!("{json}");
}
