use r2_probes::suite::extraction;
use r2_probes::trial::Record;

fn field(record: &Record, key: &str) -> Option<f64> {
    record
        .fields
        .iter()
        .find(|field| field.key == key)
        .and_then(|field| field.value.as_f64())
}

#[test]
fn an_extraction_record_saves_the_text_whose_bytes_it_counts() {
    let pieces = vec!["Row  one".to_string(), " two\tthree ".to_string()];
    let record = extraction("list-1", &pieces, 10.0, 12.0, 20.0, (1920, 1080));
    let saved = record.detail["text"].as_str().expect("saved text");
    assert_eq!(saved, "Row  one  two\tthree ");
    assert_eq!(field(&record, "stored_bytes"), Some(saved.len() as f64));
}

#[test]
fn the_error_rate_still_folds_white_space_in_the_saved_text() {
    let known = r2_probes::pages::known_text("text-1").expect("text-1");
    let words: Vec<String> = known
        .split_whitespace()
        .map(|word| format!("{word}  "))
        .collect();
    let record = extraction("text-1", &words, 10.0, 12.0, 20.0, (1920, 1080));
    assert_eq!(field(&record, "cer_pct"), Some(0.0));
    assert_eq!(
        field(&record, "stored_bytes"),
        Some(words.join(" ").len() as f64)
    );
}
