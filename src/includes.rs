//! Where NASM looks for `%include`d files: the including file's folder,
//! then the include paths (`-I`) in the `arguments` setting and in
//! `.asm-lsp.toml`'s `compile_flags_txt`, whose relative paths are from the
//! open folder, as when a build runs NASM there.

/// Splits NASM arguments into the include paths they name (`-Ipath`,
/// `-I path`, and `-i` alike) and the rest.
pub fn split(args: &[String]) -> (Vec<String>, Vec<String>) {
  let mut paths = Vec::new();
  let mut rest = Vec::new();
  let mut args = args.iter();
  while let Some(arg) = args.next() {
    match arg.strip_prefix("-I").or_else(|| arg.strip_prefix("-i")) {
      Some("") => paths.extend(args.next().cloned()),
      Some(path) => paths.push(path.to_owned()),
      None => rest.push(arg.clone()),
    }
  }
  (paths, rest)
}

/// The include paths `.asm-lsp.toml` gives NASM for a file: from the
/// `[[project]]` whose `path` holds the file most closely, else from
/// `[default_config]`, and only when its `compiler` is NASM (the flags are
/// for that compiler). `relative` is the file's path from the open folder.
pub fn from_asm_lsp(config: &str, relative: &str) -> Vec<String> {
  let Ok(config) = config.parse::<toml::Table>() else {
    return Vec::new();
  };
  let file = relative.replace('\\', "/").to_ascii_lowercase();
  let under = |path: &str| {
    let path = path
      .replace('\\', "/")
      .trim_start_matches("./")
      .trim_end_matches('/')
      .to_ascii_lowercase();
    path.is_empty() || path == "." || file == path || file.starts_with(&format!("{path}/"))
  };
  let project = config
    .get("project")
    .and_then(toml::Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(toml::Value::as_table)
    .filter_map(|project| Some((project.get("path")?.as_str()?, project)))
    .filter(|(path, _)| under(path))
    .max_by_key(|(path, _)| path.len())
    .map(|(_, project)| project);
  let chosen = project.or_else(|| config.get("default_config")?.as_table());
  let Some(opts) = chosen.and_then(|config| config.get("opts")?.as_table()) else {
    return Vec::new();
  };
  let is_nasm = opts
    .get("compiler")
    .and_then(toml::Value::as_str)
    .is_some_and(|compiler| {
      let name = compiler.rsplit(['/', '\\']).next().unwrap_or(compiler);
      name.eq_ignore_ascii_case("nasm") || name.eq_ignore_ascii_case("nasm.exe")
    });
  if !is_nasm {
    return Vec::new();
  }
  let flags: Vec<String> = opts
    .get("compile_flags_txt")
    .and_then(toml::Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(|flag| flag.as_str().map(str::to_owned))
    .collect();
  split(&flags).0
}

/// A file's path from `root` with `/`s, if it's inside.
pub fn relative_to<'a>(path: &'a str, root: &str) -> Option<&'a str> {
  let root = root.trim_end_matches(['/', '\\']);
  if path.len() <= root.len() + 1 {
    return None;
  }
  let (head, tail) = path.split_at(root.len());
  let same = if crate::is_windows_path(root) {
    head
      .replace('\\', "/")
      .eq_ignore_ascii_case(&root.replace('\\', "/"))
  } else {
    head == root
  };
  (same && tail.starts_with(['/', '\\'])).then(|| &tail[1..])
}

/// An include path as NASM takes it: absolute (relative ones are from
/// `base`) and ending with a separator, which NASM puts names straight
/// after.
pub fn absolute(base: &str, path: &str) -> String {
  let mut path = crate::join(base, path);
  if !path.ends_with(['/', '\\']) {
    path.push(if crate::is_windows_path(&path) {
      '\\'
    } else {
      '/'
    });
  }
  path
}

#[cfg(test)]
mod tests {
  use super::*;

  fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
  }

  #[test]
  fn splits_include_paths_from_other_arguments() {
    let (paths, rest) = split(&strings(&[
      "-w+all",
      "-I",
      "boot/include/",
      "-Ilib",
      "-ishared/",
      "-DDEBUG",
    ]));
    assert_eq!(paths, ["boot/include/", "lib", "shared/"]);
    assert_eq!(rest, ["-w+all", "-DDEBUG"]);
  }

  #[test]
  fn reads_nasm_include_paths_from_asm_lsp_config() {
    let config = r#"
[default_config]
assembler = "nasm"

[default_config.opts]
compiler = "nasm"
compile_flags_txt = ["-f", "bin", "-w+all", "-I", "boot/include/", "-o", "NUL"]

[[project]]
path = "kernel/asm"
assembler = "nasm"
[project.opts]
compiler = "C:/NASM/nasm.exe"
compile_flags_txt = ["-Ikernel/include/"]
"#;
    assert_eq!(from_asm_lsp(config, "boot/stage1.asm"), ["boot/include/"]);
    assert_eq!(
      from_asm_lsp(config, r"kernel\asm\entry.asm"),
      ["kernel/include/"]
    );
    assert_eq!(from_asm_lsp(config, "kernel/asmx.asm"), ["boot/include/"]);
    // Flags for another compiler, or none, give nothing.
    let gcc = "[default_config.opts]\ncompile_flags_txt = [\"-Iinc\"]\n";
    assert!(from_asm_lsp(gcc, "a.asm").is_empty());
    assert!(from_asm_lsp("not toml [", "a.asm").is_empty());
  }

  #[test]
  fn paths_are_relative_to_the_open_folder_and_end_with_a_separator() {
    assert_eq!(
      relative_to(
        r"D:\Projects\FlareOS\boot\stage1.asm",
        r"d:\projects\flareos"
      ),
      Some(r"boot\stage1.asm")
    );
    assert_eq!(
      relative_to("/home/me/os/a.asm", "/home/me/os/"),
      Some("a.asm")
    );
    assert_eq!(relative_to("/home/me/osx/a.asm", "/home/me/os"), None);
    assert_eq!(
      absolute(r"D:\Projects\FlareOS", "boot/include/"),
      r"D:\Projects\FlareOS\boot\include\"
    );
    assert_eq!(absolute("/home/me/os", "inc"), "/home/me/os/inc/");
    assert_eq!(absolute("/home/me/os", "/opt/inc/"), "/opt/inc/");
  }
}
