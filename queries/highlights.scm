; NASM, after the grammar's own highlights.scm (naclsn/tree-sitter-nasm),
; with Flareite's captures: labels, macros, and %define'd names get their
; own colors, and a directive's arguments aren't colored as keywords.
; Narrower captures win, and earlier patterns win for the same node, so
; the names come before the generic `word`.

(comment) @comment

; Names where they're defined.
(label
  name: (word) @label)
(label) @label
(preproc_multiline_macro
  name: (word) @function.macro)
(preproc_multiline_unmacro
  name: (word) @function.macro)
(preproc_def
  name: (word) @macro)
(preproc_function_def
  name: (word) @function.macro)
(preproc_undef
  name: (word) @macro)
(preproc_multiline_macro_arg_spec) @number
(struc_declaration
  name: (word) @type)
(struc_instance
  name: (word) @type)

(actual_instruction
  instruction: (word) @function)
(call_syntax_expression
  base: (word) @function)

(instruction_prefix) @keyword
(size_hint) @type
(effective_address
  hint: _ @type)
(effective_address
  segment: _ @variable.builtin)
(register) @variable.builtin

[
  (line_here_token)
  (section_here_token)
] @variable.builtin

(number_literal) @number
(packed_bcd_literal) @number
(float_literal) @float
(string_literal) @string

; Macro parameters (%1, %0) and NASM's own names (__?NASM_VER?__).
(preproc_expression) @macro
((word) @constant.builtin
  (#match? @constant.builtin "^__\\?[A-Za-z_0-9]+\\?__$"))
; Names in capitals are constants by convention.
((word) @constant
  (#match? @constant "^[A-Z_][?A-Z_0-9]+$"))
(word) @variable

(unary_expression
  operator: _ @operator)
(binary_expression
  operator: _ @operator)
(conditional_expression
  "?" @operator
  ":" @operator)

[
  ":"
  ","
] @punctuation.delimiter

[
  "("
  ")"
  "["
  "]"
  "{"
  "}"
] @punctuation.bracket

; Directives: the node is the keyword's; its names and values have their
; own captures above, which are narrower.
[
  (preproc_def)
  (preproc_function_def)
  (preproc_undef)
  (preproc_alias)
  (preproc_multiline_macro)
  (preproc_multiline_unmacro)
  (preproc_if)
  (preproc_rotate)
  (preproc_rep_loop)
  (preproc_include)
  (preproc_pathsearch)
  (preproc_depend)
  (preproc_use)
  (preproc_push)
  (preproc_pop)
  (preproc_repl)
  (preproc_stacksize)
  (preproc_local)
  (preproc_reporting)
  (preproc_pragma)
  (preproc_line)
  (preproc_clear)
] @keyword

; A directive's value is text, not a keyword.
(preproc_arg) @string.special

[
  (pseudo_instruction_dx)
  (pseudo_instruction_resx)
  (pseudo_instruction_incbin_command)
  (pseudo_instruction_equ_command)
  (pseudo_instruction_times_prefix)
  (pseudo_instruction_alignx_macro)
] @keyword

[
  (assembl_directive_target)
  (assembl_directive_defaults)
  (assembl_directive_sections)
  (assembl_directive_absolute)
  (assembl_directive_symbols)
  (assembl_directive_common)
  (assembl_directive_symbolfixes)
  (assembl_directive_cpu)
  (assembl_directive_floathandling)
  (assembl_directive_org)
  (assembl_directive_sectalign)
  (assembl_directive_primitive_target)
  (assembl_directive_primitive_defaults)
  (assembl_directive_primitive_sections)
  (assembl_directive_primitive_absolute)
  (assembl_directive_primitive_symbols)
  (assembl_directive_primitive_common)
  (assembl_directive_primitive_symbolfixes)
  (assembl_directive_primitive_cpu)
  (assembl_directive_primitive_floathandling)
  (assembl_directive_primitive_org)
  (assembl_directive_primitive_sectalign)
  (assembl_directive_primitive_warning)
  (assembl_directive_primitive_map)
] @keyword
