mod chinese_translation;
mod identity;
mod eraser;

use std::borrow::Cow;

pub use chinese_translation::ChineseTranslationPreProcessor;
pub use identity::IdentityPreProcessor;
pub use eraser::Eraser;

#[allow(dead_code)]
pub struct ProcessedText<'a> {
    pub(crate) processed: Cow<'a, str>,
    pub(crate) original: &'a str,
}

pub trait PreProcessor: Sync + Send {
    fn process<'a>(&self, s: &'a str) -> ProcessedText<'a>;
}

impl<T> PreProcessor for Box<T>
where
    T: PreProcessor
{
    fn process<'a>(&self, s: &'a str) -> ProcessedText<'a> {
        self.as_ref().process(s)
    }
}
