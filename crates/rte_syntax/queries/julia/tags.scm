; Symbols for Go to Symbol, written for RoyiTextEditor (the grammar ships no tags query).

(function_definition
  (signature
    (call_expression
      .
      (identifier) @name))) @definition.function

(function_definition
  (signature
    (typed_expression
      (call_expression
        .
        (identifier) @name)))) @definition.function

(function_definition
  (signature
    (where_expression
      (call_expression
        .
        (identifier) @name)))) @definition.function

(macro_definition
  (signature
    (call_expression
      .
      (identifier) @name))) @definition.macro

(struct_definition
  (type_head
    (identifier) @name)) @definition.struct

(struct_definition
  (type_head
    (parametrized_type_expression
      .
      (identifier) @name))) @definition.struct

(abstract_definition
  (type_head
    (identifier) @name)) @definition.type

(module_definition
  name: (identifier) @name) @definition.module
