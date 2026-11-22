//! NASM for Flareite.
//!
//! - **Problems from NASM.** When a NASM document opens, and each time a
//!   file in the open folder is saved (the extension watches the folder),
//!   NASM checks the open NASM files that are saved on disk: `nasm -X gnu
//!   -f <format> -I <folder> <file> -o <null device>`. Its errors and
//!   warnings go to Problems on their lines; those in an included file go
//!   on the `%include` line, saying where. Needs a trusted folder, as every
//!   program does.
//! - **Completion and hover** for what a file and the files it `%include`s
//!   define: macros, single-line macros, `equ` constants, structures, and
//!   labels (`symbols`), with the comments above them. asm-lsp, which the
//!   manifest declares, does instructions, registers, and directives.
//! - **asm-lsp's configuration.** asm-lsp takes its settings only from
//!   `.asm-lsp.toml`; without one it treats files as GNU assembler and
//!   reports gcc's errors. The extension offers once per folder to create
//!   one for NASM, with asm-lsp's diagnostics off (NASM's come from here).

mod report;
mod symbols;

use std::collections::{BTreeMap, HashMap};

use flareite_extension_api::events::Event;
use flareite_extension_api::language_features::{
  CompletionItem, CompletionKind, CompletionRequest, HoverRequest,
};
use flareite_extension_api::processes::{self, FinishedProcess, ProcessError, ProcessOptions};
use flareite_extension_api::prompts::{self, AnswerValue, ConfirmOptions, PromptAnswer};
use flareite_extension_api::storage::{self, StorageScope};
use flareite_extension_api::types::{
  Activation, CommandContext, ConfigValue, Diagnostic, DocumentId, DocumentInfo, LogLevel,
  MessageLevel, Position, Range, Severity,
};
use flareite_extension_api::workspace::FileChanges;
use flareite_extension_api::{Extension, files, host, language_servers, register, workspace};

use report::{Level, Report};
use symbols::{Kind, Outline, Symbol};

const LANGUAGE: &str = "nasm";
const CONFIG_FILE: &str = ".asm-lsp.toml";
/// What `.asm-lsp.toml` says: NASM's documentation for x86 and x86-64, and
/// no diagnostics, which NASM itself gives.
const ASM_LSP_CONFIG: &str = "\
# asm-lsp's settings (https://github.com/bergercookie/asm-lsp), made by
# Flareite's NASM extension: NASM's directives and x86 instructions for
# hover and completion. The extension runs NASM for errors and warnings, so
# asm-lsp's own diagnostics (from gcc) are off.
[default_config]
assembler = \"nasm\"
instruction_set = \"x86/x86-64\"

[default_config.opts]
diagnostics = false
default_diagnostics = false
";
/// Workspace storage: the user declined `.asm-lsp.toml` here.
const DECLINED_KEY: &str = "asm-lsp-config-declined";

/// An open NASM document.
struct Open {
  path: String,
  version: u64,
}

/// NASM running on a document.
struct Check {
  document: DocumentId,
  /// The document's version when it started, which the problems are for.
  version: u64,
  /// Where it ran: the file's folder.
  folder: String,
}

#[derive(Default)]
struct Nasm {
  documents: BTreeMap<DocumentId, Open>,
  checks: HashMap<u64, Check>,
  /// The newest check of each document; older ones' results are dropped.
  latest: HashMap<DocumentId, u64>,
  watch: Option<u64>,
  /// The `.asm-lsp.toml` prompt showing, if one is.
  config_prompt: Option<u64>,
  /// Folders asked about this session.
  asked: Vec<String>,
  /// Whether "NASM isn't installed" was said this session.
  told_missing: bool,
}

fn log(message: &str) {
  host::log(LogLevel::Info, message);
}

fn setting_bool(key: &str) -> bool {
  !matches!(host::configuration(key), Some(ConfigValue::Boolean(false)))
}

fn setting_text(key: &str) -> String {
  match host::configuration(key) {
    Some(ConfigValue::Text(text)) => text,
    _ => String::new(),
  }
}

/// The folder a path is in, and its name.
fn split_path(path: &str) -> (&str, &str) {
  match path.rfind(['/', '\\']) {
    Some(i) => (&path[..i], &path[i + 1..]),
    None => ("", path),
  }
}

fn is_windows_path(path: &str) -> bool {
  path.as_bytes().get(1) == Some(&b':') || path.starts_with("\\\\")
}

fn join(folder: &str, relative: &str) -> String {
  if relative.starts_with(['/', '\\']) || is_windows_path(relative) {
    return relative.to_owned();
  }
  let separator = if is_windows_path(folder) { '\\' } else { '/' };
  let relative = relative.trim_start_matches("./").trim_start_matches(".\\");
  let relative: String = if separator == '\\' {
    relative.replace('/', "\\")
  } else {
    relative.to_owned()
  };
  format!("{folder}{separator}{relative}")
}

fn same_path(a: &str, b: &str) -> bool {
  let normal = |path: &str| path.replace('\\', "/").to_ascii_lowercase();
  if is_windows_path(a) {
    normal(a) == normal(b)
  } else {
    a == b
  }
}

/// Reads a whole document.
fn document_text(id: DocumentId) -> Option<(u64, String)> {
  let info = host::document(id).ok()?;
  let slice = host::read_lines(id, 0, info.line_count).ok()?;
  Some((slice.version, slice.lines.join("\n")))
}

/// The output format to check a file for.
fn format_for(path: &str, text: &str) -> String {
  let chosen = setting_text("format");
  if !chosen.is_empty() && chosen != "auto" {
    return chosen;
  }
  let flat = text.lines().any(|line| {
    let code = line[..symbols::comment_start(line).unwrap_or(line.len())]
      .trim()
      .to_ascii_lowercase();
    let code = code.trim_start_matches('[').trim_start();
    code.starts_with("org ") || code.starts_with("bits 16")
  });
  if flat {
    "bin".into()
  } else if is_windows_path(path) {
    "win64".into()
  } else if path.starts_with("/Users/") {
    "macho64".into()
  } else {
    "elf64".into()
  }
}

/// The arguments NASM runs with for a file.
fn arguments(path: &str, text: &str) -> Vec<String> {
  let (folder, name) = split_path(path);
  let separator = if is_windows_path(path) { "\\" } else { "/" };
  let mut args = vec![
    "-X".into(),
    "gnu".into(),
    "-f".into(),
    format_for(path, text),
    format!("-I{folder}{separator}"),
  ];
  args.extend(
    setting_text("arguments")
      .split_whitespace()
      .map(str::to_owned),
  );
  args.push(name.into());
  args.push("-o".into());
  args.push(
    if is_windows_path(path) {
      "NUL"
    } else {
      "/dev/null"
    }
    .into(),
  );
  args
}

/// The length of a document's line, in bytes (the API's columns).
fn line_length(lines: &[&str], line: u32) -> u32 {
  lines.get(line as usize).map_or(0, |text| text.len() as u32)
}

/// Turns NASM's reports into problems in the file it checked.
fn diagnostics(reports: &[Report], path: &str, folder: &str, text: &str) -> Vec<Diagnostic> {
  let lines: Vec<&str> = text.lines().collect();
  let outline = symbols::outline(text);
  let whole_line = |line: u32| Range {
    start: Position { line, column: 0 },
    end: Position {
      line,
      column: line_length(&lines, line),
    },
  };
  reports
    .iter()
    .map(|report| {
      let mut message = report.message.clone();
      let here = report
        .file
        .as_deref()
        .is_some_and(|file| same_path(&join(folder, file), path));
      let line = if here {
        report.line.saturating_sub(1)
      } else if let Some(file) = &report.file {
        // In an included file: on the line that includes it, if there's one.
        let (_, name) = split_path(file);
        message = format!("{file}:{}: {message}", report.line);
        outline
          .includes
          .iter()
          .find(|(include, _)| split_path(include).1 == name)
          .map_or(0, |&(_, line)| line)
      } else {
        0
      };
      for (file, number, context) in &report.context {
        message.push_str(&format!("\n{file}:{number}: {context}"));
      }
      Diagnostic {
        range: whole_line(line.min(lines.len().saturating_sub(1) as u32)),
        severity: match report.level {
          Level::Error => Severity::Error,
          Level::Warning => Severity::Warning,
        },
        message,
        code: report.code.clone(),
      }
    })
    .collect()
}

/// Reads a file the extension may read, if it can.
fn read(path: &str) -> Option<String> {
  files::read_file(path).ok()
}

impl Nasm {
  fn watch_folder(&mut self) {
    if let Some(watch) = self.watch.take() {
      workspace::unwatch(watch);
    }
    if let Some(folder) = workspace::folder() {
      match workspace::watch(&folder.path) {
        Ok(watch) => self.watch = Some(watch),
        Err(error) => log(&format!("can't watch the folder for saves: {error:?}")),
      }
    }
  }

  fn opened(&mut self, info: &DocumentInfo) {
    if info.language.as_deref() != Some(LANGUAGE) {
      return;
    }
    let Some(path) = info.path.clone() else {
      return;
    };
    self.documents.insert(
      info.id,
      Open {
        path: path.clone(),
        version: info.version,
      },
    );
    self.check(info.id);
    self.offer_config(&path);
  }

  /// Runs NASM on a document's file, if it's saved and checking is on.
  fn check(&mut self, id: DocumentId) {
    let Some(open) = self.documents.get(&id) else {
      return;
    };
    if !setting_bool("diagnostics") {
      let _ = host::publish_diagnostics(id, open.version, &[]);
      return;
    }
    if workspace::folder().is_some_and(|folder| !folder.trusted) {
      return;
    }
    let path = open.path.clone();
    let Some((version, text)) = document_text(id) else {
      return;
    };
    let (folder, _) = split_path(&path);
    let options = ProcessOptions {
      cwd: Some(folder.to_owned()),
      env: Vec::new(),
      stdin: None,
      timeout_ms: Some(30_000),
    };
    match processes::spawn("nasm", &arguments(&path, &text), &options) {
      Ok(process) => {
        self.checks.insert(
          process,
          Check {
            document: id,
            version,
            folder: folder.to_owned(),
          },
        );
        self.latest.insert(id, process);
      }
      Err(ProcessError::NotFound(why)) => {
        if !self.told_missing {
          self.told_missing = true;
          host::show_message(
            MessageLevel::Warning,
            &format!(
              "NASM couldn't be run to check your files ({why}). Install it from nasm.us and put it on the PATH, or set where it is in settings.toml: [extensions.programs] \"flareite.nasm.nasm\" = \"C:/path/to/nasm.exe\"."
            ),
          );
        }
      }
      Err(ProcessError::Untrusted) => {}
      Err(error) => log(&format!("NASM couldn't be run: {error:?}")),
    }
  }

  fn finished(&mut self, finished: FinishedProcess) {
    let Some(check) = self.checks.remove(&finished.process) else {
      return;
    };
    if self.latest.get(&check.document) != Some(&finished.process) {
      return;
    }
    let Some(open) = self.documents.get(&check.document) else {
      return;
    };
    let output = match finished.outcome {
      Ok(output) => output,
      Err(error) => {
        log(&format!("NASM failed: {error:?}"));
        return;
      }
    };
    let mut text = String::from_utf8_lossy(&output.stderr).into_owned();
    text.push('\n');
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    let reports = report::parse(&text);
    // The problems are for the text NASM read: the file as saved, which is
    // the document's text unless it changed since.
    let Some((_, current)) = document_text(check.document) else {
      return;
    };
    let found = diagnostics(&reports, &open.path, &check.folder, &current);
    if let Err(error) = host::publish_diagnostics(check.document, check.version, &found) {
      log(&format!("couldn't show NASM's problems: {error:?}"));
    }
  }

  /// Checks again the documents a save may have changed: those saved, and
  /// every one if an include file was.
  fn files_changed(&mut self, changes: &FileChanges) {
    let assembly = |path: &str| {
      let lower = path.to_ascii_lowercase();
      [".asm", ".nasm", ".inc", ".mac"]
        .iter()
        .any(|extension| lower.ends_with(extension))
    };
    let included = changes.overflow
      || changes.paths.iter().any(|path| {
        let lower = path.to_ascii_lowercase();
        lower.ends_with(".inc") || lower.ends_with(".mac")
      });
    let to_check: Vec<DocumentId> = self
      .documents
      .iter()
      .filter(|(_, open)| {
        included
          || changes
            .paths
            .iter()
            .any(|path| assembly(path) && same_path(path, &open.path))
      })
      .map(|(&id, _)| id)
      .collect();
    for id in to_check {
      self.check(id);
    }
  }

  /// Asks once per folder to create `.asm-lsp.toml`, if it has none.
  fn offer_config(&mut self, path: &str) {
    if self.config_prompt.is_some() || !setting_bool("offer_asm_lsp_config") {
      return;
    }
    let Some(folder) = workspace::folder() else {
      return;
    };
    let inside = path
      .to_ascii_lowercase()
      .replace('\\', "/")
      .starts_with(&folder.path.to_ascii_lowercase().replace('\\', "/"));
    if !inside || self.asked.contains(&folder.path) {
      return;
    }
    self.asked.push(folder.path.clone());
    if storage::get(StorageScope::Workspace, DECLINED_KEY).is_some()
      || read(&join(&folder.path, CONFIG_FILE)).is_some()
    {
      return;
    }
    let options = ConfirmOptions {
      message: "Set up asm-lsp for NASM in this folder?".into(),
      detail: Some(
        "asm-lsp reads its settings from .asm-lsp.toml. Without one, it describes GNU assembler (GAS) directives and reports gcc's errors on NASM code. Create one that sets NASM, with asm-lsp's own error checking off (this extension runs NASM for that)?".into(),
      ),
      buttons: vec!["Create .asm-lsp.toml".into(), "Don't Ask Again".into()],
      destructive: false,
    };
    match prompts::confirm(&options) {
      Ok(prompt) => self.config_prompt = Some(prompt),
      Err(error) => log(&format!("couldn't ask about .asm-lsp.toml: {error:?}")),
    }
  }

  fn answered(&mut self, answer: PromptAnswer) -> Result<(), String> {
    if self.config_prompt != Some(answer.prompt) {
      return Ok(());
    }
    self.config_prompt = None;
    match answer.value {
      AnswerValue::Button(0) => create_config(),
      AnswerValue::Button(1) => storage::set(StorageScope::Workspace, DECLINED_KEY, Some("true"))
        .map_err(|error| format!("{error:?}")),
      _ => Ok(()),
    }
  }

  /// What a document and the files it includes define: theirs, and those
  /// files' names.
  fn symbols(&self, id: DocumentId) -> Vec<(Symbol, Option<String>)> {
    let Some((_, text)) = document_text(id) else {
      return Vec::new();
    };
    let folder = self
      .documents
      .get(&id)
      .map(|open| split_path(&open.path).0.to_owned());
    let mut found: Vec<(Symbol, Option<String>)> = Vec::new();
    let outline = symbols::outline(&text);
    let mut pending: Vec<(String, Outline)> = Vec::new();
    if let Some(folder) = &folder {
      for (include, _) in &outline.includes {
        let path = join(folder, include);
        if let Some(text) = read(&path) {
          pending.push((include.clone(), symbols::outline(&text)));
        }
      }
    }
    found.extend(outline.symbols.into_iter().map(|symbol| (symbol, None)));
    // Their own includes, one level further, with paths from the same folder.
    let mut nested = Vec::new();
    if let Some(folder) = &folder {
      for (_, outline) in &pending {
        for (include, _) in &outline.includes {
          if pending.iter().any(|(seen, _)| seen == include) {
            continue;
          }
          if let Some(text) = read(&join(folder, include)) {
            nested.push((include.clone(), symbols::outline(&text)));
          }
        }
      }
    }
    for (file, outline) in pending.into_iter().chain(nested) {
      found.extend(
        outline
          .symbols
          .into_iter()
          .map(|symbol| (symbol, Some(file.clone()))),
      );
    }
    found
  }
}

fn create_config() -> Result<(), String> {
  let folder = workspace::folder().ok_or("no folder is open")?;
  let path = join(&folder.path, CONFIG_FILE);
  files::write_file(&path, ASM_LSP_CONFIG).map_err(|error| format!("{error:?}"))?;
  // asm-lsp reads it when it starts.
  let _ = language_servers::restart("asm-lsp");
  host::show_message(
    MessageLevel::Info,
    "Created .asm-lsp.toml: asm-lsp now describes NASM. Edit it to change the instruction set.",
  );
  Ok(())
}

fn completion_kind(kind: Kind) -> CompletionKind {
  match kind {
    Kind::Macro => CompletionKind::Function,
    Kind::Define | Kind::Constant => CompletionKind::Constant,
    Kind::Struct => CompletionKind::Struct,
    Kind::Label => CompletionKind::Variable,
  }
}

/// A symbol described in Markdown, for hover.
fn describe(symbol: &Symbol, file: Option<&str>) -> String {
  let mut text = format!("```nasm\n{}\n```\n", symbol.source);
  let mut what = symbol.kind.describe().to_owned();
  if let Some(parameters) = &symbol.parameters {
    what = format!("{what}, {parameters} parameters");
  }
  let place = match file {
    Some(file) => format!("{file}, line {}", symbol.line + 1),
    None => format!("line {}", symbol.line + 1),
  };
  text.push_str(&format!("\n*{what}* · {place}"));
  if !symbol.doc.is_empty() {
    text.push_str("\n\n");
    text.push_str(&symbol.doc);
  }
  text
}

/// Converts a column (UTF-8 bytes, as the API counts) to a character index.
fn char_column(line: &str, column: u32) -> usize {
  line
    .char_indices()
    .take_while(|&(offset, _)| offset < column as usize)
    .count()
}

/// Converts a character index to a column (UTF-8 bytes).
fn byte_column(line: &str, index: usize) -> u32 {
  line.chars().take(index).map(char::len_utf8).sum::<usize>() as u32
}

fn line_of(id: DocumentId, line: u32) -> Option<String> {
  host::read_lines(id, line, line + 1)
    .ok()?
    .lines
    .into_iter()
    .next()
}

impl Extension for Nasm {
  fn activate(_: Activation) -> Result<Self, String> {
    let mut nasm = Nasm::default();
    nasm.watch_folder();
    Ok(nasm)
  }

  fn run_command(&mut self, command: &str, context: CommandContext) -> Result<(), String> {
    match command {
      "flareite.nasm.check" => {
        let id = context.document.ok_or("Open a NASM file to check it.")?;
        if !self.documents.contains_key(&id) {
          return Err("The file in use isn't a saved NASM file.".into());
        }
        if workspace::folder().is_some_and(|folder| !folder.trusted) {
          return Err("NASM runs only in a folder you trust.".into());
        }
        self.check(id);
        Ok(())
      }
      "flareite.nasm.createAsmLspConfig" => {
        let folder = workspace::folder().ok_or("Open a folder to configure asm-lsp in it.")?;
        if read(&join(&folder.path, CONFIG_FILE)).is_some() {
          let _ = workspace::open_file(&join(&folder.path, CONFIG_FILE));
          return Ok(());
        }
        create_config()
      }
      other => Err(format!("unknown command {other}")),
    }
  }

  fn handle_event(&mut self, event: Event) -> Result<(), String> {
    match event {
      Event::DocumentOpened(info) => self.opened(&info),
      Event::DocumentChanged(change) => {
        if let Some(open) = self.documents.get_mut(&change.document) {
          open.version = change.version;
        }
      }
      Event::DocumentClosed(id) => {
        self.documents.remove(&id);
        self.latest.remove(&id);
      }
      Event::ProcessFinished(finished) => self.finished(finished),
      Event::FilesChanged(changes) if Some(changes.watch) == self.watch => {
        self.files_changed(&changes)
      }
      Event::FolderChanged => {
        self.watch_folder();
        let ids: Vec<_> = self.documents.keys().copied().collect();
        for id in ids {
          self.check(id);
        }
      }
      Event::ConfigurationChanged => {
        let ids: Vec<_> = self.documents.keys().copied().collect();
        for id in ids {
          self.check(id);
        }
      }
      Event::PromptAnswered(answer) => self.answered(answer)?,
      _ => {}
    }
    Ok(())
  }

  fn provide_completions(
    &mut self,
    request: CompletionRequest,
  ) -> Result<Vec<CompletionItem>, String> {
    let Some(line) = line_of(request.document, request.position.line) else {
      return Ok(Vec::new());
    };
    let column = char_column(&line, request.position.column);
    let before: String = line.chars().take(column).collect();
    // Not in a comment.
    if symbols::comment_start(&before).is_some() {
      return Ok(Vec::new());
    }
    let (start, prefix) = symbols::prefix_at(&line, column);
    let start = Position {
      line: request.position.line,
      column: byte_column(&line, start),
    };
    let mut seen = std::collections::HashSet::new();
    let items = self
      .symbols(request.document)
      .into_iter()
      .filter(|(symbol, _)| {
        // Local labels only after a dot; others whatever is typed.
        prefix.starts_with('.') == symbol.name.starts_with('.') && seen.insert(symbol.name.clone())
      })
      .map(|(symbol, file)| {
        let mut detail = symbol.kind.describe().to_owned();
        if let Some(file) = file {
          detail = format!("{detail} · {file}");
        }
        CompletionItem {
          label: symbol.name.clone(),
          kind: completion_kind(symbol.kind),
          detail: Some(detail),
          start,
          insert: symbol.name.clone(),
          filter: None,
          sort: None,
          deprecated: false,
        }
      })
      .take(1000)
      .collect();
    Ok(items)
  }

  fn provide_hover(&mut self, request: HoverRequest) -> Result<Option<String>, String> {
    let Some(line) = line_of(request.document, request.position.line) else {
      return Ok(None);
    };
    let column = char_column(&line, request.position.column);
    // Comment starts are bytes, as the request's column is.
    if symbols::comment_start(&line).is_some_and(|start| start <= request.position.column as usize)
    {
      return Ok(None);
    }
    let Some((_, word)) = symbols::word_at(&line, column) else {
      return Ok(None);
    };
    // A local label is the one nearest above, in this file.
    let symbols = self.symbols(request.document);
    let found = if word.starts_with('.') {
      symbols
        .iter()
        .rfind(|(symbol, file)| {
          file.is_none() && symbol.is(&word) && symbol.line <= request.position.line
        })
        .or_else(|| symbols.iter().find(|(symbol, _)| symbol.is(&word)))
    } else {
      symbols.iter().find(|(symbol, _)| symbol.is(&word))
    };
    Ok(found.map(|(symbol, file)| describe(symbol, file.as_deref())))
  }
}

register!(Nasm);

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn joins_paths_in_the_folders_style() {
    assert_eq!(join("C:\\code", "inc/defs.inc"), "C:\\code\\inc\\defs.inc");
    assert_eq!(join("/home/me", "./defs.inc"), "/home/me/defs.inc");
    assert_eq!(join("/home/me", "/abs.inc"), "/abs.inc");
    assert!(same_path("C:\\Code\\a.asm", "c:/code/A.asm"));
    assert!(!same_path("/a/A.asm", "/a/a.asm"));
  }

  #[test]
  fn puts_problems_on_their_lines_and_included_ones_on_the_include() {
    let text = "%include \"defs.inc\"\nmov eax, 1\nbad\n";
    let reports = report::parse(
      "hello.asm:3: error: invalid combination of opcode and operands\n\
       defs.inc:4: ... from macro `bad' defined here\n\
       defs.inc:2: warning: label alone on a line [-w+label-orphan]\n\
       nasm: fatal: something\n",
    );
    let found = diagnostics(&reports, "C:\\p\\hello.asm", "C:\\p", text);
    assert_eq!(found.len(), 3);
    assert_eq!(found[0].range.start.line, 2);
    assert_eq!(found[0].range.end.column, 3);
    assert_eq!(
      found[0].message,
      "invalid combination of opcode and operands\ndefs.inc:4: from macro `bad' defined here"
    );
    assert_eq!(found[1].range.start.line, 0);
    assert_eq!(found[1].severity, Severity::Warning);
    assert_eq!(found[1].message, "defs.inc:2: label alone on a line");
    assert_eq!(found[1].code.as_deref(), Some("label-orphan"));
    assert_eq!(found[2].range.start.line, 0);
  }

  #[test]
  fn describes_symbols() {
    let outline = symbols::outline("; Prints.\n%macro print 2\n%endmacro\n");
    assert_eq!(
      describe(&outline.symbols[0], Some("io.inc")),
      "```nasm\n%macro print 2\n```\n\n*macro, 2 parameters* · io.inc, line 2\n\nPrints."
    );
  }

  #[test]
  fn converts_columns() {
    assert_eq!(char_column("a😀b", 5), 2);
    assert_eq!(byte_column("a😀b", 2), 5);
  }
}
