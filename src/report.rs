//! NASM's errors and warnings, as it writes them with `-X gnu`:
//!
//! ```text
//! hello.asm:18: error: invalid combination of opcode and operands
//! defs.inc:4: ... from macro `bad' defined here
//! hello.asm:19: warning: word data exceeds bounds [-w+number-overflow]
//! nasm: fatal: unable to open include file `missing.inc'
//! ```
//!
//! A line starting `...` adds where the one before came from.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
  Error,
  Warning,
}

/// One of NASM's reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
  /// As NASM wrote it (relative to where it ran); `None` for one about no
  /// file in particular.
  pub file: Option<String>,
  /// 1-based; 0 when NASM gave none.
  pub line: u32,
  pub level: Level,
  pub message: String,
  /// The warning's class, such as `number-overflow`.
  pub code: Option<String>,
  /// Where it came from, such as a macro: `(file, line, text)`.
  pub context: Vec<(String, u32, String)>,
}

/// `file:line: rest`, with `file` possibly holding a drive's colon.
fn locate(line: &str) -> Option<(&str, u32, &str)> {
  let mut search = 0;
  while let Some(found) = line[search..].find(':') {
    let colon = search + found;
    let after = &line[colon + 1..];
    let digits = after.len() - after.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits > 0 && after[digits..].starts_with(": ") {
      let number = after[..digits].parse().ok()?;
      return Some((&line[..colon], number, &after[digits + 2..]));
    }
    search = colon + 1;
  }
  None
}

fn level_of(text: &str) -> Option<(Level, &str)> {
  for (prefix, level) in [
    ("error: ", Level::Error),
    ("fatal: ", Level::Error),
    ("panic: ", Level::Error),
    ("warning: ", Level::Warning),
  ] {
    if let Some(rest) = text.strip_prefix(prefix) {
      return Some((level, rest));
    }
  }
  None
}

/// Splits `[-w+class]` off the end of a warning.
fn code_of(message: &str) -> (String, Option<String>) {
  if let Some(open) = message.rfind(" [-w+")
    && message.ends_with(']')
  {
    let code = &message[open + 5..message.len() - 1];
    return (message[..open].to_owned(), Some(code.to_owned()));
  }
  (message.to_owned(), None)
}

/// Reads NASM's output.
pub fn parse(output: &str) -> Vec<Report> {
  let mut reports: Vec<Report> = Vec::new();
  for line in output.lines() {
    let line = line.trim_end();
    if let Some((file, number, rest)) = locate(line) {
      if let Some(context) = rest.strip_prefix("... ") {
        if let Some(last) = reports.last_mut() {
          last
            .context
            .push((file.to_owned(), number, context.to_owned()));
        }
        continue;
      }
      if let Some((level, message)) = level_of(rest) {
        let (message, code) = code_of(message);
        reports.push(Report {
          file: Some(file.to_owned()),
          line: number,
          level,
          message,
          code,
          context: Vec::new(),
        });
      }
      continue;
    }
    // `nasm: fatal: ...`, about no line.
    if let Some(rest) = line.strip_prefix("nasm: ")
      && let Some((level, message)) = level_of(rest)
    {
      let (message, code) = code_of(message);
      reports.push(Report {
        file: None,
        line: 0,
        level,
        message,
        code,
        context: Vec::new(),
      });
    }
  }
  reports
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reads_errors_warnings_and_where_they_came_from() {
    let reports = parse(
      "hello.asm:18: error: invalid combination of opcode and operands\n\
       defs.inc:4: ... from macro `bad' defined here\n\
       C:\\code\\hello.asm:19: warning: word data exceeds bounds [-w+number-overflow]\n\
       nasm: fatal: unable to open include file `missing.inc'\n\
       something else\n",
    );
    assert_eq!(reports.len(), 3);
    assert_eq!(reports[0].file.as_deref(), Some("hello.asm"));
    assert_eq!(reports[0].line, 18);
    assert_eq!(reports[0].level, Level::Error);
    assert_eq!(
      reports[0].context,
      [(
        "defs.inc".to_owned(),
        4,
        "from macro `bad' defined here".to_owned()
      )]
    );
    assert_eq!(reports[1].file.as_deref(), Some("C:\\code\\hello.asm"));
    assert_eq!(reports[1].level, Level::Warning);
    assert_eq!(reports[1].message, "word data exceeds bounds");
    assert_eq!(reports[1].code.as_deref(), Some("number-overflow"));
    assert_eq!(reports[2].file, None);
    assert_eq!(
      reports[2].message,
      "unable to open include file `missing.inc'"
    );
  }
}
