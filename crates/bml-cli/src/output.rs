use bml_core::semantic::diagnostics::{Diagnostic, Severity};

/// Print diagnostics to stderr in a human-readable format.
/// Returns the count of errors.
pub fn print_diagnostics(diagnostics: &[Diagnostic], source: &str, filename: &str) -> usize {
    let mut error_count = 0;

    for diag in diagnostics {
        if diag.severity == Severity::Error {
            error_count += 1;
        }

        let severity_label = match diag.severity {
            Severity::Error => "\x1b[31merror\x1b[0m",
            Severity::Warning => "\x1b[33mwarning\x1b[0m",
            Severity::Info => "\x1b[34minfo\x1b[0m",
        };

        eprintln!("{}[{}]: {}", severity_label, diag.code, diag.message);
        eprintln!(
            "  \x1b[36m-->\x1b[0m {}:{}:{}",
            filename, diag.span.start.line, diag.span.start.column
        );

        // Show the source line
        if let Some(line) = source.lines().nth(diag.span.start.line - 1) {
            eprintln!("   \x1b[90m|\x1b[0m");
            eprintln!(" \x1b[90m{} |\x1b[0m {}", diag.span.start.line, line);
            // Underline
            let padding = " ".repeat(diag.span.start.column - 1);
            let underline_len = if diag.span.start.line == diag.span.end.line {
                (diag.span.end.column - diag.span.start.column).max(1)
            } else {
                line.len().saturating_sub(diag.span.start.column - 1).max(1)
            };
            let underline_char = if diag.severity == Severity::Error {
                "^"
            } else {
                "~"
            };
            eprintln!(
                "   \x1b[90m|\x1b[0m {}{}",
                padding,
                underline_char.repeat(underline_len)
            );
        }

        if let Some(help) = &diag.help {
            eprintln!("   \x1b[90m= help:\x1b[0m {}", help);
        }
        eprintln!();
    }

    error_count
}

/// Print diagnostics for a multi-file project. `sources` is indexed by span
/// file id (`Project::sources()`); a diagnostic without one falls back to the
/// file named in `Diagnostic::file`, then to the only file of the project.
/// Returns the count of errors.
pub fn print_diagnostics_multi_file(
    diagnostics: &[Diagnostic],
    sources: &[(String, &str)],
) -> usize {
    let mut error_count = 0;

    for diag in diagnostics {
        if diag.severity == Severity::Error {
            error_count += 1;
        }

        let found = by_file_id(sources, diag.span.file_id).or(match &diag.file {
            Some(file) => sources.iter().find(|(name, _)| name == file),
            None if sources.len() == 1 => sources.first(),
            None => None,
        });
        let (filename, source) = found
            .map(|(f, s)| (f.as_str(), *s))
            .unwrap_or(("<project>", ""));

        print_single_diagnostic(diag, source, filename, sources);
    }

    error_count
}

/// Source for a span file id (1-based; 0 means unknown).
fn by_file_id<'a, 's>(
    sources: &'a [(String, &'s str)],
    file_id: u32,
) -> Option<&'a (String, &'s str)> {
    sources.get((file_id as usize).checked_sub(1)?)
}

/// Print a single diagnostic with source context.
fn print_single_diagnostic(
    diag: &Diagnostic,
    source: &str,
    filename: &str,
    sources: &[(String, &str)],
) {
    let severity_label = match diag.severity {
        Severity::Error => "\x1b[31merror\x1b[0m",
        Severity::Warning => "\x1b[33mwarning\x1b[0m",
        Severity::Info => "\x1b[34minfo\x1b[0m",
    };

    eprintln!("{}[{}]: {}", severity_label, diag.code, diag.message);
    eprintln!(
        "  \x1b[36m-->\x1b[0m {}:{}:{}",
        filename, diag.span.start.line, diag.span.start.column
    );

    // Show the source line
    if let Some(line) = source.lines().nth(diag.span.start.line - 1) {
        eprintln!("   \x1b[90m|\x1b[0m");
        eprintln!(" \x1b[90m{} |\x1b[0m {}", diag.span.start.line, line);
        // Underline
        let padding = " ".repeat(diag.span.start.column - 1);
        let underline_len = if diag.span.start.line == diag.span.end.line {
            (diag.span.end.column - diag.span.start.column).max(1)
        } else {
            line.len().saturating_sub(diag.span.start.column - 1).max(1)
        };
        let underline_char = if diag.severity == Severity::Error {
            "^"
        } else {
            "~"
        };
        eprintln!(
            "   \x1b[90m|\x1b[0m {}{}",
            padding,
            underline_char.repeat(underline_len)
        );
    }

    if let Some(help) = &diag.help {
        eprintln!("   \x1b[90m= help:\x1b[0m {}", help);
    }

    // Show related information
    for related in &diag.related {
        let related_file = by_file_id(sources, related.span.file_id).map_or(filename, |(f, _)| f);
        eprintln!(
            "  \x1b[36m-->\x1b[0m related: {}:{}:{}",
            related_file, related.span.start.line, related.span.start.column
        );
        eprintln!("   \x1b[90m= note:\x1b[0m {}", related.message);
    }

    eprintln!();
}

/// Print a summary of diagnostics.
pub fn print_summary(error_count: usize, warning_count: usize) {
    if error_count > 0 {
        eprintln!(
            "\x1b[31merror\x1b[0m: aborting due to {} error{}{}",
            error_count,
            if error_count == 1 { "" } else { "s" },
            if warning_count > 0 {
                format!(
                    "; {} warning{} emitted",
                    warning_count,
                    if warning_count == 1 { "" } else { "s" }
                )
            } else {
                String::new()
            }
        );
    } else if warning_count > 0 {
        eprintln!(
            "\x1b[33mwarning\x1b[0m: {} warning{} emitted",
            warning_count,
            if warning_count == 1 { "" } else { "s" }
        );
    }
}
