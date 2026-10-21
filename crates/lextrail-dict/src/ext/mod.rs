//! Extended dictionaries (the Oxford dictionary, D049): rules shared by the converter
//! (`tools/oxford-builder`) and the reader, so that they exist only once (D038).

pub mod id;
pub mod text;

/// Source name of the Oxford Advanced Learner's English-Chinese Dictionary, 10th edition.
pub const OXFORD_SOURCE: &str = "oalecd10";
