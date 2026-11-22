# NASM

[NASM](https://nasm.us) assembly for Flareite: `.asm`, `.nasm`, `.inc`, and `.mac` files.

- **Highlighting** from a Tree-sitter grammar
  ([naclsn/tree-sitter-nasm](https://github.com/naclsn/tree-sitter-nasm)): instructions,
  registers, labels, macros and `%define`d names, directives, numbers, and strings. Macros,
  `%if` blocks, `%rep` loops, and structures fold.
- **NASM's errors and warnings** in Problems, on their lines, each time you save a file (and when
  one opens): the extension runs `nasm -X gnu -f <format>` on it. Problems in an included file
  show on the `%include` line, saying where; those from inside a macro say which. **NASM: Check
  File with NASM** runs it now. NASM runs only in folders you trust.
- **Include paths.** NASM runs in the open folder, as a build would, and looks for `%include`d
  files next to the file, then in the `-I` paths of the `arguments` setting and of
  `.asm-lsp.toml`'s `compile_flags_txt` (when its `compiler` is `nasm`), relative to the open
  folder. Completion and hover find included files the same way. So a project that builds with
  `nasm -I boot/include/ ...` needs `arguments = "-I boot/include/"` under
  `[extensions.settings."flareite.nasm"]` in its `.flareite/settings.toml`, or nothing more if
  its `.asm-lsp.toml` already says so.
- **Your macros, `%define`s, `equ` constants, structures, and labels**, of the file and the
  files it `%include`s, complete as you type, and hovering over one shows its definition and the
  comment above it. Local labels (`.loop`) complete after a `.`.
- **[asm-lsp](https://github.com/bergercookie/asm-lsp)** describes x86 instructions, registers,
  and NASM's directives on hover and in completion, goes to definitions (F12), and lists the
  file's labels (Ctrl+Shift+O). Install it with `cargo install asm-lsp`. It reads its settings
  only from `.asm-lsp.toml`, and without one it treats files as GNU assembler and reports gcc's
  errors, so the extension offers to create one for NASM in the open folder, with asm-lsp's own
  error checking off (**NASM: Create asm-lsp Configuration** does it any time).

## Settings

| Setting                | Default | What it does                                                                                                                              |
| ---------------------- | ------- | ----------------------------------------------------------------------------------------------------------------------------------------- |
| `diagnostics`          | `true`  | Run NASM on files as they open and are saved.                                                                                             |
| `format`               | `auto`  | The output format to check for (`-f`). `auto` is `bin` for files with `org` or `bits 16`, else the system's 64-bit format (win64, elf64). |
| `arguments`            | (none)  | More arguments for NASM, such as `-w+all -DDEBUG -I include/`, with paths relative to the open folder (see Include paths).                |
| `offer_asm_lsp_config` | `true`  | Offer to create `.asm-lsp.toml` in a folder without one.                                                                                  |

If NASM isn't on the PATH, set where it is in `settings.toml`:

```toml
[extensions.programs]
"flareite.nasm.nasm" = "C:/Program Files/NASM/nasm.exe"
```
