use std::borrow::Cow;

use std::collections::HashMap;
use crate::internal_tokenizer::InternalTokenizer;
use crate::Token;
use once_cell::sync::Lazy;
use crate::internal_tokenizer::{UnicodeSegmenter, TokenStream};

pub trait PreProcessor: Sync + Send {
    fn process<'a>(&self, s: &'a str) -> Cow<'a, str>;
}

type Pipeline = (Box<dyn PreProcessor>, Box<dyn InternalTokenizer>, Box<dyn Normalizer>);

static DEFAULT_ANALYZER: Lazy<Pipeline> = Lazy::new(||
    (Box::new(IdentityPreProcessor), Box::new(UnicodeSegmenter), Box::new(IdentityNormalizer)));

struct IdentityPreProcessor;

impl PreProcessor for IdentityPreProcessor {
    fn process<'a>(&self, s: &'a str) -> Cow<'a, str> {
        Cow::Borrowed(s)
    }
}

pub trait Normalizer: Sync + Send {
    fn normalize<'a>(&self, token: Token<'a>) -> Token<'a>;
}

struct IdentityNormalizer;

impl Normalizer for IdentityNormalizer {
    fn normalize<'a>(&self, token: Token<'a>) -> Token<'a> {
        token
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Language;
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Script;

#[derive(Default)]
pub struct AnalyzerConfig {
    pub tokenizer_map: HashMap<(Script, Language), Pipeline>,
}

pub struct Analyzer {
    /// script specialized tokenizer, this can be switched during
    /// document tokenization if the document contains several scripts
    tokenizer_map: HashMap<(Script, Language), Pipeline>,
}

pub struct AnalyzedText<'a> {
    /// Reference to the original text
    text: &'a str,
    /// Processed text
    processed: Cow<'a, str>,
    /// Pipeline used to proccess the text
    pipeline: &'a Pipeline,
}

impl<'a> AnalyzedText<'a> {
    /// Returns a `TokenStream` for the Analyzed text.
    pub fn tokens(&'a self) -> TokenStream<'a> {
        let stream = self.pipeline.1
            .tokenize(self.processed.as_ref())
            .map(move |t| self.pipeline.2.normalize(t));
        TokenStream {
            inner: Box::new(stream)
        }
    }

    /// Attaches each token to its corresponding portion of the original text.
    pub fn reconstruct(&'a self) -> impl Iterator<Item = (&'a str, Token<'a>)> {
        self.tokens().map(move |t| (&self.text[t.byte_start..t.byte_end], t))
    } 
}

impl Analyzer {
    /// create a new tokenizer detecting script
    /// and chose the specialized internal tokenizer
    pub fn new(config: AnalyzerConfig) -> Self {
        Self {
            tokenizer_map: config.tokenizer_map,
        }
    }

    /// Builds an `AnalyzedText` instance with the correct analyzer pipeline, and pre-processes the
    /// text. E.G:
    /// ```rust
    /// use meilisearch_tokenizer::{Analyzer, AnalyzerConfig};
    /// // defaults to unicode segmenter with identity preprocessor and normalizer.
    /// let analyzer = Analyzer::new(AnalyzerConfig::default());
    /// let analyzed = analyzer.analyze("The quick (\"brown\") fox can't jump 32.3 feet, right? Brr, it's 29.3°F!");
    /// let mut tokens = analyzed.tokens();
    /// assert!("The" == tokens.next().unwrap().text());
    /// ```
    pub fn analyze<'a>(&'a self, text: &'a str) -> AnalyzedText<'a> { 
        let pipeline = self.tokenizer_map.get(&(Script, Language)).unwrap_or_else(|| &*DEFAULT_ANALYZER);
        let processed = pipeline.0.process(text);
        AnalyzedText {
            text,
            processed,
            pipeline,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_simple() {
        let analyzer = Analyzer::new(AnalyzerConfig::default());
        let orig = "The quick (\"brown\") fox can't jump 32.3 feet, right? Brr, it's 29.3°F!";
        let tokens = analyzer.analyze(orig);
        assert_eq!(orig, tokens.tokens().map(|t| &orig[t.byte_start..t.byte_end]).collect::<String>());
    }
}