use crate::compiler::SourceFileInfo;

#[allow(dead_code)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
    pub primary: Label,
    pub secondary: Vec<Label>,
    pub notes: Vec<String>,
    pub help: Vec<String>,
}

#[allow(dead_code)]
pub struct SpanInfo {
    pub file: SourceFileInfo,
    pub start: usize,
    pub end: usize,
}

#[allow(dead_code)]
pub struct Label {
    pub span: SpanInfo,
    pub message: Option<String>,
}

#[allow(dead_code)]
pub struct DiagnosticCode {
    pub code: u32,
    pub category: &'static str,
}

#[allow(dead_code)]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}
