(string) @string

(member
  name: (_) @string.special.key)

(number) @number

(null) @constant.builtin

[
  (true)
  (false)
] @boolean

(comment) @comment

[
  "{"
  "}"
  "["
  "]"
] @punctuation.bracket

[
  ","
  ":"
] @punctuation.delimiter
