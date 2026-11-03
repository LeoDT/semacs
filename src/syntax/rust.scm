(const_item name: (identifier) @constant)
(static_item name: (identifier) @constant)
(integer_literal) @number
(float_literal) @number

[
  "+" "-" "/" "%" "=" "==" "!=" "<" "<=" ">" ">="
  "&&" "||" "!" "^" "|" "<<" ">>" "?"
  "+=" "-=" "*=" "/=" "%=" "&=" "|=" "^=" "<<=" ">>="
] @operator

["->" "=>" ".." "..=" "..."] @punctuation.delimiter
