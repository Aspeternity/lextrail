//! Extended dictionaries (the Oxford dictionary, D049): rules shared by the converter
//! (`tools/oxford-builder`) and the reader, so that they exist only once (D038).

pub mod id;
pub mod schema;
pub mod text;

/// Source name of the Oxford Advanced Learner's English-Chinese Dictionary, 10th edition.
pub const OXFORD_SOURCE: &str = "oalecd10";

/// File name of the Oxford dictionary in the extended dictionary folder (spec 2.2).
pub const OXFORD_FILE_NAME: &str = "oalecd10.db";
/// File name of its audio file.
pub const OXFORD_AUDIO_FILE_NAME: &str = "oalecd10-audio.db";
