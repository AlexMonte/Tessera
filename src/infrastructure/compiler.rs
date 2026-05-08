use crate::application::{
    compile_container, compile_normalized_program, normalize_program, resolve_spatial_program,
    validate_program_shape,
};
use crate::domain::{
    AuthoredTesseraProgram, ContainerId, CycleSpan, Diagnostic, DiagnosticCategory, DiagnosticKind,
    DiagnosticLocation, NormalizedProgram, PatternIr, TesseraProgram,
};

use super::{CompileReport, PreviewReport, ValidationReport};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompileOptions {
    pub validate_before_compile: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            validate_before_compile: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TesseraCompiler {
    options: CompileOptions,
}

impl TesseraCompiler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(options: CompileOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &CompileOptions {
        &self.options
    }

    pub fn validate(&self, program: &TesseraProgram) -> ValidationReport {
        if let Err(diagnostics) = validate_program_shape(program) {
            return ValidationReport::invalid(diagnostics);
        }

        match normalize_program(program) {
            Ok(normalized) => ValidationReport::valid(normalized),
            Err(diagnostics) => ValidationReport::invalid(diagnostics),
        }
    }

    pub fn normalize(
        &self,
        program: &TesseraProgram,
    ) -> Result<NormalizedProgram, Vec<Diagnostic>> {
        validate_program_shape(program)?;
        normalize_program(program)
    }

    pub fn compile(&self, program: &TesseraProgram) -> Result<CompileReport, Vec<Diagnostic>> {
        if self.options.validate_before_compile {
            validate_program_shape(program)?;
        }
        let normalized = normalize_program(program)?;
        let ir = compile_normalized_program(&normalized)?;
        Ok(CompileReport { normalized, ir })
    }

    pub fn compile_ir(&self, program: &TesseraProgram) -> Result<PatternIr, Vec<Diagnostic>> {
        self.compile(program).map(|report| report.ir)
    }

    pub fn preview_container(
        &self,
        program: &TesseraProgram,
        container_id: ContainerId,
        span: CycleSpan,
    ) -> Result<PreviewReport, Vec<Diagnostic>> {
        let normalized_program = self.normalize(program)?;
        let Some(container) = normalized_program.containers.get(&container_id) else {
            return Err(vec![Diagnostic::new(
                DiagnosticCategory::Placement,
                DiagnosticKind::MissingContainer,
                "Container preview target is missing from normalized program.",
                Some(DiagnosticLocation::ContainerStack {
                    container: container_id,
                    index: 0,
                }),
            )]);
        };
        let stream = compile_container(&normalized_program, container, span, 0)?;
        Ok(PreviewReport { stream })
    }

    pub fn resolve(
        &self,
        authored: &AuthoredTesseraProgram,
    ) -> Result<TesseraProgram, Vec<Diagnostic>> {
        resolve_spatial_program(authored)
    }

    pub fn validate_authored(&self, authored: &AuthoredTesseraProgram) -> ValidationReport {
        match self.resolve(authored) {
            Ok(resolved) => self.validate(&resolved),
            Err(diagnostics) => ValidationReport::invalid(diagnostics),
        }
    }

    pub fn compile_authored(
        &self,
        authored: &AuthoredTesseraProgram,
    ) -> Result<CompileReport, Vec<Diagnostic>> {
        let resolved = self.resolve(authored)?;
        self.compile(&resolved)
    }

    pub fn compile_authored_ir(
        &self,
        authored: &AuthoredTesseraProgram,
    ) -> Result<PatternIr, Vec<Diagnostic>> {
        Ok(self.compile_authored(authored)?.ir)
    }
}
