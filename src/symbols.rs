//! The names a NASM file defines: multi-line macros, single-line macros
//! (`%define`, `%assign`, ...), `equ` constants, structures, and labels;
//! and the files it `%include`s. Read line by line, which is how NASM's
//! preprocessor reads them too.

/// What a name is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
  Macro,
  Define,
  Constant,
  Struct,
  Label,
}

impl Kind {
  pub fn describe(self) -> &'static str {
    match self {
      Kind::Macro => "macro",
      Kind::Define => "single-line macro",
      Kind::Constant => "constant",
      Kind::Struct => "structure",
      Kind::Label => "label",
    }
  }
}

/// A name a file defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
  pub name: String,
  pub kind: Kind,
  /// 0-based.
  pub line: u32,
  /// The defining line, without its comment.
  pub source: String,
  /// The comment lines just above it, and one at the end of its line.
  pub doc: String,
  /// `%imacro`, `%idefine`: matched in any case.
  pub case_insensitive: bool,
  /// For a macro, how many parameters it takes ("2", "1-3", "0+").
  pub parameters: Option<String>,
}

/// What a file defines and includes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Outline {
  pub symbols: Vec<Symbol>,
  /// `%include` paths as written, with the 0-based line of each.
  pub includes: Vec<(String, u32)>,
}

/// Where a line's comment starts, outside strings: `None` if it has none.
pub fn comment_start(line: &str) -> Option<usize> {
  let mut quote = None;
  for (i, c) in line.char_indices() {
    match quote {
      Some(q) if c == q => quote = None,
      Some(_) => {}
      None if c == '"' || c == '\'' || c == '`' => quote = Some(c),
      None if c == ';' => return Some(i),
      None => {}
    }
  }
  None
}

pub fn is_name_start(c: char) -> bool {
  c.is_ascii_alphabetic() || matches!(c, '_' | '.' | '?' | '$' | '@')
}

pub fn is_name_char(c: char) -> bool {
  c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '?' | '$' | '@' | '#' | '~')
}

/// A name at the start of `text`, and the rest.
fn split_name(text: &str) -> Option<(&str, &str)> {
  let text = text.trim_start();
  let first = text.chars().next()?;
  if !is_name_start(first) {
    return None;
  }
  let end = text
    .char_indices()
    .find(|&(_, c)| !is_name_char(c))
    .map_or(text.len(), |(i, _)| i);
  Some((&text[..end], &text[end..]))
}

/// The first word of `text` (letters, digits, `%`, and `_`), lowercased.
fn keyword(text: &str) -> String {
  text
    .trim_start()
    .chars()
    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '%' | '_'))
    .collect::<String>()
    .to_ascii_lowercase()
}

const DATA: &[&str] = &[
  "db", "dw", "dd", "dq", "dt", "do", "dy", "dz", "resb", "resw", "resd", "resq", "rest", "reso",
  "resy", "resz", "times", "incbin",
];

const DEFINES: &[(&str, bool)] = &[
  ("%define", false),
  ("%xdefine", false),
  ("%idefine", true),
  ("%ixdefine", true),
  ("%assign", false),
  ("%iassign", true),
  ("%defstr", false),
  ("%idefstr", true),
  ("%deftok", false),
  ("%ideftok", true),
  ("%strlen", false),
  ("%substr", false),
  ("%strcat", false),
];

/// Reads what `text` defines.
pub fn outline(text: &str) -> Outline {
  let mut outline = Outline::default();
  // Comment lines just above, for the next definition.
  let mut comments: Vec<String> = Vec::new();
  for (number, raw) in text.lines().enumerate() {
    let number = number as u32;
    let (code, comment) = match comment_start(raw) {
      Some(i) => (&raw[..i], Some(raw[i + 1..].trim())),
      None => (raw, None),
    };
    let code_trimmed = code.trim();
    if code_trimmed.is_empty() {
      match comment {
        Some(comment) => comments.push(comment.to_owned()),
        None => comments.clear(),
      }
      continue;
    }
    let mut doc = std::mem::take(&mut comments);
    if let Some(comment) = comment.filter(|c| !c.is_empty()) {
      doc.push(comment.to_owned());
    }
    let symbol = |name: &str, kind, case_insensitive, parameters: Option<String>| Symbol {
      name: name.to_owned(),
      kind,
      line: number,
      source: code_trimmed.to_owned(),
      doc: doc.join("\n"),
      case_insensitive,
      parameters,
    };
    let word = keyword(code_trimmed);
    if word.starts_with('%') {
      let rest = code_trimmed[word.len()..].trim_start();
      if word == "%macro" || word == "%imacro" {
        if let Some((name, after)) = split_name(rest) {
          let parameters = after.split_whitespace().next().map(str::to_owned);
          outline
            .symbols
            .push(symbol(name, Kind::Macro, word == "%imacro", parameters));
        }
      } else if let Some(&(_, insensitive)) = DEFINES.iter().find(|(d, _)| *d == word) {
        if let Some((name, _)) = split_name(rest) {
          outline
            .symbols
            .push(symbol(name, Kind::Define, insensitive, None));
        }
      } else if word == "%include" {
        let path = rest.trim().trim_matches(['"', '\'', '`', '<', '>']);
        if !path.is_empty() {
          outline.includes.push((path.to_owned(), number));
        }
      }
      continue;
    }
    if word == "struc" {
      if let Some((name, _)) = split_name(&code_trimmed[5..]) {
        outline
          .symbols
          .push(symbol(name, Kind::Struct, false, None));
      }
      continue;
    }
    // A label: a name with a colon, or one followed by data or `equ`.
    let Some((name, after)) = split_name(code_trimmed) else {
      continue;
    };
    let colon = after.trim_start().starts_with(':');
    let after = if colon {
      &after.trim_start()[1..]
    } else {
      after
    };
    let next = keyword(after);
    if next == "equ" {
      outline
        .symbols
        .push(symbol(name, Kind::Constant, false, None));
    } else if colon || DATA.contains(&next.as_str()) {
      outline.symbols.push(symbol(name, Kind::Label, false, None));
    }
  }
  outline
}

impl Symbol {
  /// Whether this is the symbol `word` names.
  pub fn is(&self, word: &str) -> bool {
    if self.case_insensitive {
      self.name.eq_ignore_ascii_case(word)
    } else {
      self.name == word
    }
  }
}

/// The name around a column (0-based, in characters) of a line, with where
/// it starts; `%` and `%%` before it count (`%%loop`), as do macro
/// parameters such as `%1`.
pub fn word_at(line: &str, column: usize) -> Option<(usize, String)> {
  let chars: Vec<char> = line.chars().collect();
  let column = column.min(chars.len());
  let mut start = column;
  while start > 0 && (is_name_char(chars[start - 1]) || chars[start - 1] == '%') {
    start -= 1;
  }
  let mut end = column;
  while end < chars.len() && is_name_char(chars[end]) {
    end += 1;
  }
  (end > start).then(|| (start, chars[start..end].iter().collect()))
}

/// The name typed before a column, and where it starts: what completion
/// replaces. Empty at a word's start.
pub fn prefix_at(line: &str, column: usize) -> (usize, String) {
  let chars: Vec<char> = line.chars().collect();
  let column = column.min(chars.len());
  let mut start = column;
  while start > 0 && (is_name_char(chars[start - 1]) || chars[start - 1] == '%') {
    start -= 1;
  }
  (start, chars[start..column].iter().collect())
}

#[cfg(test)]
mod tests {
  use super::*;

  const SOURCE: &str = "\
%include \"defs.inc\"
; Prints a message.
; Takes the address and the length.
%macro print 2
  mov rdx, %2
%endmacro
%idefine Width 80 ; columns

section .data
message: db \"Hi; there\", 10
.len equ $ - message
buffer resb 64
SIZE equ 64

struc point
  .x: resd 1
endstruc

section .text
_start:
  print message, 6
  jmp _start
";

  #[test]
  fn finds_macros_defines_constants_structures_and_labels() {
    let outline = outline(SOURCE);
    let names: Vec<_> = outline
      .symbols
      .iter()
      .map(|s| (s.name.as_str(), s.kind, s.line))
      .collect();
    assert_eq!(
      names,
      [
        ("print", Kind::Macro, 3),
        ("Width", Kind::Define, 6),
        ("message", Kind::Label, 9),
        (".len", Kind::Constant, 10),
        ("buffer", Kind::Label, 11),
        ("SIZE", Kind::Constant, 12),
        ("point", Kind::Struct, 14),
        (".x", Kind::Label, 15),
        ("_start", Kind::Label, 19),
      ]
    );
    assert_eq!(outline.includes, [("defs.inc".to_owned(), 0)]);
    let print = &outline.symbols[0];
    assert_eq!(
      print.doc,
      "Prints a message.\nTakes the address and the length."
    );
    assert_eq!(print.parameters.as_deref(), Some("2"));
    let width = &outline.symbols[1];
    assert_eq!(width.doc, "columns");
    assert_eq!(width.source, "%idefine Width 80");
    assert!(width.is("WIDTH") && !outline.symbols[0].is("PRINT"));
    // Blank lines part comments from what follows.
    assert_eq!(outline.symbols[2].doc, "");
  }

  #[test]
  fn ignores_semicolons_in_strings() {
    assert_eq!(comment_start("db \"a;b\" ; c"), Some(9));
    assert_eq!(comment_start("db 'a;b'"), None);
  }

  #[test]
  fn finds_the_word_at_a_column() {
    assert_eq!(word_at("  jmp .loop", 7), Some((6, ".loop".into())));
    assert_eq!(word_at("  mov eax, %%x", 13), Some((11, "%%x".into())));
    assert_eq!(word_at("  mov eax, ebx", 9), Some((6, "eax".into())));
    assert_eq!(word_at("  ", 1), None);
    assert_eq!(prefix_at("  pri", 5), (2, "pri".into()));
    assert_eq!(prefix_at("  mov ", 6), (6, String::new()));
  }
}
