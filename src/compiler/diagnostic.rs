use crate::{
    common::location::{LocationInfo, Span},
    compiler::SourceFileInfo,
};

pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub primary: Label,
    pub secondary: Vec<Label>,
    pub notes: Vec<String>,
    pub help: Vec<String>,
}

pub struct SpanInfo {
    pub file: SourceFileInfo,
    pub start: usize,
    pub end: usize,
}

pub struct Label {
    pub span: SpanInfo,
    pub message: Option<String>,
}

pub struct DiagnosticCode {
    pub code: u32,
    pub category: &'static str,
}

pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}
