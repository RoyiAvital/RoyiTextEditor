; Symbols for Go to Symbol, written for RoyiTextEditor (the grammar ships no tags query).

(create_table
  (object_reference
    name: (identifier) @name)) @definition.table

(create_view
  (object_reference
    name: (identifier) @name)) @definition.view

(create_function
  (object_reference
    name: (identifier) @name)) @definition.function
