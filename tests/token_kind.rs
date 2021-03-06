use fst::Set;

use meilisearch_tokenizer::{Analyzer, AnalyzerConfig};
use meilisearch_tokenizer::token::SeparatorKind;

#[test]
fn test() {
    let stop_words = Set::from_iter(["of".as_bytes(), "the".as_bytes()].iter()).unwrap();
    let analyzer = Analyzer::new(AnalyzerConfig::default_with_stopwords(&stop_words));
    let analyzed = analyzer.analyze("Hello, the dog.");
    let mut tokens = analyzed.tokens();
    assert!(tokens.next().unwrap().is_word());
    assert_eq!(tokens.next().unwrap().is_separator(), Some(SeparatorKind::Hard));
    assert!(tokens.next().unwrap().is_stopword());
    assert_eq!(tokens.next().unwrap().is_separator(), Some(SeparatorKind::Soft));
    assert!(tokens.next().unwrap().is_word());
    assert_eq!(tokens.next().unwrap().is_separator(), Some(SeparatorKind::Hard));
    assert!(tokens.next().is_none());
}
