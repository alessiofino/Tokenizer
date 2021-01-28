use std::collections::HashMap;

use fst::Set;
use once_cell::sync::Lazy;

use crate::detection::is_cjk;
use crate::normalizer::{DeunicodeNormalizer, IdentityNormalizer, LowercaseNormalizer, Normalizer};
use crate::processors::{
    ChineseTranslationPreProcessor, IdentityPreProcessor, PreProcessor, ProcessedText,
};
use crate::token_classifier::TokenClassifier;
use crate::tokenizer::{Jieba, LegacyMeilisearch, TokenStream, Tokenizer, UnicodeSegmenter};
use crate::Token;

static DEFAULT_PIPELINE: Lazy<Pipeline> = Lazy::new(Pipeline::default);

pub struct Pipeline {
    pre_processor: Box<dyn PreProcessor + 'static>,
    tokenizer: Box<dyn Tokenizer + 'static>,
    normalizer: Box<dyn Normalizer + 'static>,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self {
            pre_processor: Box::new(IdentityPreProcessor),
            tokenizer: Box::new(UnicodeSegmenter),
            normalizer: Box::new(IdentityNormalizer),
        }
    }
}

impl Pipeline {
    pub fn set_pre_processor(mut self, pre_processor: impl PreProcessor + 'static) -> Self {
        self.pre_processor = Box::new(pre_processor);
        self
    }

    pub fn set_tokenizer(mut self, tokenizer: impl Tokenizer + 'static) -> Self {
        self.tokenizer = Box::new(tokenizer);
        self
    }

    pub fn set_normalizer(mut self, normalizer: impl Normalizer + 'static) -> Self {
        self.normalizer = Box::new(normalizer);
        self
    }
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum Language {
    English,
    Other,
}

macro_rules! make_script {
    ($($script:tt), +) => {
        #[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
        pub enum Script {
            $($script),+,
            Other,
        }

        impl From<whatlang::Script> for Script {
            fn from(other: whatlang::Script) -> Script {
                match other {
                    $(whatlang::Script::$script => Script::$script), +
                }
            }

        }
    };
}

make_script! {
    Arabic,
    Bengali,
    Cyrillic,
    Devanagari,
    Ethiopic,
    Georgian,
    Greek,
    Gujarati,
    Gurmukhi,
    Hangul,
    Hebrew,
    Hiragana,
    Kannada,
    Katakana,
    Khmer,
    Latin,
    Malayalam,
    Mandarin,
    Myanmar,
    Oriya,
    Sinhala,
    Tamil,
    Telugu,
    Thai
}

pub struct AnalyzerConfig<A> {
    /// language specialized tokenizer, this can be switched during
    /// document tokenization if the document contains several languages
    pub pipeline_map: HashMap<(Script, Language), Pipeline>,
    pub stop_words: Set<A>,
}

impl<A> AnalyzerConfig<A>
where
    A: AsRef<[u8]>,
{
    pub fn default_with_stopwords(stop_words: Set<A>) -> Self {
        let mut pipeline_map: HashMap<(Script, Language), Pipeline> = HashMap::new();

        // Latin script specialized pipeline
        let latin_normalizer: Vec<Box<dyn Normalizer>> = vec![
            Box::new(DeunicodeNormalizer::default()),
            Box::new(LowercaseNormalizer),
        ];
        pipeline_map.insert(
            (Script::Latin, Language::Other),
            Pipeline::default()
                .set_tokenizer(LegacyMeilisearch)
                .set_normalizer(latin_normalizer),
        );

        // Chinese script specialized pipeline
        let chinese_deunicoder =
            DeunicodeNormalizer::new(&|text: &str| text.chars().next().map_or(false, is_cjk));
        let chinese_normalizer: Vec<Box<dyn Normalizer>> =
            vec![Box::new(chinese_deunicoder), Box::new(LowercaseNormalizer)];
        pipeline_map.insert(
            (Script::Mandarin, Language::Other),
            Pipeline::default()
                .set_pre_processor(ChineseTranslationPreProcessor)
                .set_tokenizer(Jieba::default())
                .set_normalizer(chinese_normalizer),
        );

        AnalyzerConfig {
            pipeline_map,
            stop_words,
        }
    }

    pub fn new(pipeline_map: HashMap<(Script, Language), Pipeline>, stop_words: Set<A>) -> Self {
        Self {
            pipeline_map,
            stop_words,
        }
    }
}

pub struct Analyzer<A> {
    config: AnalyzerConfig<A>,
}

pub struct AnalyzedText<'a, A> {
    /// Processed text
    processed: ProcessedText<'a>,
    /// Pipeline used to proccess the text
    pipeline: &'a Pipeline,
    /// Classifier used to give token a kind
    classifier: TokenClassifier<'a, A>,
}

impl<'a, A> AnalyzedText<'a, A>
where
    A: AsRef<[u8]>,
{
    /// Returns a `TokenStream` for the Analyzed text.
    pub fn tokens(&'a self) -> TokenStream<'a> {
        let stream = self
            .pipeline
            .tokenizer
            .tokenize(&self.processed)
            .map(move |t| self.pipeline.normalizer.normalize(t))
            .map(move |t| self.classifier.classify(t));
        TokenStream {
            inner: Box::new(stream),
        }
    }

    /// Attaches each token to its corresponding portion of the original text.
    pub fn reconstruct(&'a self) -> impl Iterator<Item = (&'a str, Token<'a>)> {
        self.tokens()
            .map(move |t| (&self.processed.original[t.byte_start..t.byte_end], t))
    }
}

impl<A> Analyzer<A> {
    /// create a new tokenizer detecting script
    /// and chose the specialized internal tokenizer
    pub fn new(config: AnalyzerConfig<A>) -> Self {
        Self { config }
    }

    /// Builds an `AnalyzedText` instance with the correct analyzer pipeline, and pre-processes the
    /// text.
    ///
    /// If an analysis pipeline exists for the inferred `(Script, Language)`, the analyzer will look
    /// for a user specified default `(Script::Other, Language::Other)`. If the user default is not
    /// specified, it will fallback to `(IdentityPreProcessor, UnicodeSegmenter, IdentityNormalizer)`.
    ///
    /// ```rust
    /// use meilisearch_tokenizer::{Analyzer, AnalyzerConfig};
    /// use fst::Set;
    /// // defaults to unicode segmenter with identity preprocessor and normalizer.
    /// let stop_words = Set::from_iter([""].iter()).unwrap();
    /// let analyzer = Analyzer::new(AnalyzerConfig::default_with_stopwords(&stop_words));
    /// let analyzed = analyzer.analyze("The quick (\"brown\") fox can't jump 32.3 feet, right? Brr, it's 29.3°F!");
    /// let mut tokens = analyzed.tokens();
    /// assert!("the" == tokens.next().unwrap().text());
    /// ```
    pub fn analyze<'t>(&'t self, text: &'t str) -> AnalyzedText<'t, A> {
        let pipeline = self.pipeline_from(text);
        let processed = pipeline.pre_processor.process(text);
        let classifier = TokenClassifier::new(&self.config.stop_words);

        AnalyzedText {
            processed,
            pipeline,
            classifier,
        }
    }

    /// Try to Detect Language and Script and return the corresponding pipeline,
    /// if no Language is detected or no pipeline corresponds to the Language
    /// the function try to get a pipeline corresponding to the script;
    /// if no Script is detected or no pipeline corresponds to the Script,
    /// the function try to get the default pipeline in the map;
    /// if no default pipeline exist in the map return the librairy DEFAULT_PIPELINE.
    fn pipeline_from(&self, text: &str) -> &Pipeline {
        let script = self.detect_script(text);
        let language = self.detect_lang(text);
        self.config
            .pipeline_map
            .get(&(script, language))
            .or_else(|| self.config.pipeline_map.get(&(script, Language::Other)))
            .or_else(|| {
                self.config
                    .pipeline_map
                    .get(&(Script::Other, Language::Other))
            })
            .unwrap_or_else(|| &*DEFAULT_PIPELINE)
    }

    /// detect script with whatlang,
    /// if no script is detected, return Script::Other
    fn detect_script(&self, text: &str) -> Script {
        whatlang::detect_script(text)
            .map(Script::from)
            .unwrap_or(Script::Other)
    }

    /// detect lang (dummy)
    fn detect_lang(&self, _text: &str) -> Language {
        Language::Other
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::normalizer::LowercaseNormalizer;

    #[test]
    fn test_simple_latin() {
        let stop_words = Set::default();
        let analyzer = Analyzer::new(AnalyzerConfig::default_with_stopwords(&stop_words));

        let orig = "The quick (\"brown\") fox can't jump 32.3 feet, right? Brr, it's 29.3°F!";
        let analyzed = analyzer.analyze(orig);
        let mut analyzed = analyzed.tokens();
        assert_eq!("the", analyzed.next().unwrap().text());
        assert_eq!(" ", analyzed.next().unwrap().text());
        assert_eq!("quick", analyzed.next().unwrap().text());
        assert_eq!(" (\"", analyzed.next().unwrap().text());
        assert_eq!("brown", analyzed.next().unwrap().text());
    }

    #[test]
    fn test_simple_chinese() {
        let stop_words = Set::default();
        let analyzer = Analyzer::new(AnalyzerConfig::default_with_stopwords(&stop_words));

        let orig = "人人生而自由﹐在尊严和权利上一律平等。他们赋有理性和良心﹐并应以兄弟关系的精神互相对待。";
        let analyzed = analyzer.analyze(orig);
        let analyzed: Vec<_> = analyzed.tokens().map(|token| token.word).collect();
        assert_eq!(
            analyzed,
            [
                "人人",
                "生而自由",
                ",",
                "在",
                "尊严",
                "和",
                "权利",
                "上",
                "一律平等",
                "。",
                "他们",
                "赋有",
                "理性",
                "和",
                "良心",
                ",",
                "并",
                "应以",
                "兄弟",
                "关系",
                "的",
                "精神",
                "互相",
                "对待",
                "。"
            ]
        );
    }

    #[test]
    fn test_traditional_chinese() {
        let stop_words = Set::default();
        let analyzer = Analyzer::new(AnalyzerConfig::default_with_stopwords(&stop_words));

        let traditional = "人人生而自由﹐在尊嚴和權利上一律平等。他們賦有理性和良心﹐並應以兄弟關係的精神互相對待。";
        let _simplified = "人人生而自由﹐在尊严和权利上一律平等。他们赋有理性和良心﹐并应以兄弟关系的精神互相对待。";

        let analyzed = analyzer.analyze(traditional);
        let analyzed: Vec<_> = analyzed.tokens().map(|token| token.word).collect();

        assert_eq!(
            analyzed,
            [
                "人人",
                "生而自由",
                ",",
                "在",
                "尊严",
                "和",
                "权利",
                "上",
                "一律平等",
                "。",
                "他们",
                "赋有",
                "理性",
                "和",
                "良心",
                ",",
                "并",
                "应以",
                "兄弟",
                "关系",
                "的",
                "精神",
                "互相",
                "对待",
                "。"
            ]
        );
    }

    #[test]
    fn test_simple_latin_with_lowercase_normalizer() {
        let mut pipeline_map: HashMap<(Script, Language), Pipeline> = HashMap::new();
        pipeline_map.insert(
            (Script::Latin, Language::Other),
            Pipeline::default().set_normalizer(LowercaseNormalizer),
        );

        let stop_words = Set::default();
        let analyzer = Analyzer::new(AnalyzerConfig::new(pipeline_map, &stop_words));
        let orig = "The quick (\"brown\") fox can't jump 32.3 feet, right? Brr, it's 29.3°F!";
        let analyzed = analyzer.analyze(orig);
        assert_eq!("the", analyzed.tokens().next().unwrap().text());
    }

    #[test]
    fn test_reconstruct_latin() {
        let stop_words = Set::default();
        let analyzer = Analyzer::new(AnalyzerConfig::default_with_stopwords(&stop_words));
        let orig = "The quick (\"brown\") fox can't jump 32.3 feet, right? Brr, it's 29.3°F!";
        let tokens = analyzer.analyze(orig);
        assert_eq!(
            orig,
            tokens.reconstruct().map(|(t, _)| t).collect::<String>()
        );
    }

    #[test]
    fn test_reconstruct_chinese() {
        let stop_words = Set::default();
        let analyzer = Analyzer::new(AnalyzerConfig::default_with_stopwords(&stop_words));
        let orig = "人人生而自由﹐在尊严和权利上一律平等。他们赋有理性和良心﹐并应以兄弟关系的精神互相对待。";
        let tokens = analyzer.analyze(orig);
        assert_eq!(
            orig,
            tokens.reconstruct().map(|(t, _)| t).collect::<String>()
        );
    }

    #[test]
    fn test_reconstruct_traditional_chinese() {
        let stop_words = Set::default();
        let analyzer = Analyzer::new(AnalyzerConfig::default_with_stopwords(&stop_words));
        let traditional = "人人生而自由﹐在尊嚴和權利上一律平等。他們賦有理性和良心﹐並應以兄弟關係的精神互相對待。";
        let tokens = analyzer.analyze(traditional);
        assert_eq!(
            traditional,
            tokens.reconstruct().map(|(t, _)| t).collect::<String>()
        );
    }
}
