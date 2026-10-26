; BuildingML syntax highlighting queries

; Comments
(comment) @comment
(doc_comment) @comment.documentation

; Headings
(heading_marker) @keyword
(spatial_type) @type
(heading_name) @title

; Elements
(element_type) @type
(element_id) @variable
(material_id) @variable

; Properties
(property_key) @property

; Values
(number) @number
(unit) @keyword
(string) @string
(boolean) @constant.builtin
(identifier) @variable
(profile_type) @function.builtin

; Import
"import" @keyword
"from" @keyword

; Punctuation
":" @punctuation.delimiter
"," @punctuation.delimiter
"(" @punctuation.bracket
")" @punctuation.bracket
"[" @punctuation.bracket
"]" @punctuation.bracket

; Keywords in axis
"to" @keyword
"Material" @keyword
