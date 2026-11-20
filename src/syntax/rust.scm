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
