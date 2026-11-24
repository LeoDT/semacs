(const_item name: (identifier) @constant)
(static_item name: (identifier) @constant)
(integer_literal) @number
(float_literal) @number

(self) @variable.special
(use_list (self) @variable)
(scoped_use_list (self) @variable)
(scoped_identifier (self) @variable)

[
  "+" "-" "/" "%" "=" "==" "!=" "<" "<=" ">" ">="
  "&&" "||" "!" "^" "|" "<<" ">>" "?"
  "+=" "-=" "*=" "/=" "%=" "&=" "|=" "^=" "<<=" ">>="
] @operator

["->" "=>" ".." "..=" "..."] @punctuation.delimiter

(call_expression
  function: (identifier) @function.call)
(call_expression
  function: (scoped_identifier
    name: (identifier) @function.call))
(call_expression
  function: (field_expression
    field: (field_identifier) @function.method.call))
(generic_function
  function: (identifier) @function.call)
(generic_function
  function: (scoped_identifier
    name: (identifier) @function.call))
(generic_function
  function: (field_expression
    field: (field_identifier) @function.method.call))
