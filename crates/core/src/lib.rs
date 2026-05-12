mod cleanup;
mod config;
mod diagnostics;
mod dom;
mod error;
mod extract;
mod metadata;
mod patterns;
mod readable;
mod scoring;
mod serialize;

pub use config::{Article, ReadabilityOptions, ReadableOptions};
pub use diagnostics::{
    AttemptDiagnostic, CandidateDiagnostic, CandidateSelection, CleanupDiagnostic, ContentSelectorDiagnostic,
    ExtractionDiagnostics, ExtractionOutcome, ExtractionReport, FlagDiagnostic, NodeDiagnostic,
};
pub use error::Error;
pub use extract::{extract, extract_with_diagnostics};
pub use readable::is_probably_readable;
