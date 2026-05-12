mod cleanup;
mod config;
mod diagnostics;
mod dom;
mod error;
mod extract;
mod json_schema;
mod markdown;
mod metadata;
mod normalize;
mod patterns;
mod readable;
mod recovery;
mod scoring;
mod serialize;

pub use config::{Article, ReadabilityOptions, ReadableOptions};
pub use diagnostics::{
    AttemptDiagnostic, CandidateDiagnostic, CandidateSelection, CleanupDiagnostic, ContentSelectorDiagnostic,
    ExtractionDiagnostics, ExtractionOutcome, ExtractionReport, FlagDiagnostic, NodeDiagnostic, RecoveryDiagnostic,
};
pub use error::Error;
pub use extract::{extract, extract_with_diagnostics};
pub use markdown::html_to_markdown;
pub use readable::is_probably_readable;
