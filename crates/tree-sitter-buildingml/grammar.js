/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

// BuildingML tree-sitter grammar
//
// Design: Headings (#-####) are always at column 0 and form a FLAT list.
// The CST-to-AST pass builds the spatial hierarchy from heading levels.
// INDENT/DEDENT are used for element bodies and property blocks only.
//
// External scanner tokens:
//   NEWLINE — line boundary (same indent or heading at col 0)
//   INDENT  — line boundary with increased indentation
//   DEDENT  — decreased indentation

module.exports = grammar({
  name: "buildingml",

  externals: ($) => [$._newline, $._indent, $._dedent],

  extras: (_$) => [/[ \t]/],

  conflicts: ($) => [
    // comparison_expression and in_expression share _check_operand prefix
    [$.comparison_expression, $.in_expression],
  ],

  word: ($) => $.identifier,

  rules: {
    source_file: ($) => repeat($._item),

    _item: ($) =>
      choice(
        $.heading,
        $.element_declaration,
        $.import_statement,
        $.material_definition,
        $.template_definition,
        $.check_suite_declaration,
        $.check_declaration,
        $.comment,
        $.doc_comment,
        $._newline,
      ),

    // --- Headings (spatial hierarchy) ---
    // Headings are always at column 0. Their "body" is the properties
    // that follow at indent > 0 (captured by INDENT/DEDENT).
    // Child headings are NOT indented — they appear as siblings in the CST.
    heading: ($) =>
      seq(
        $.heading_marker,
        $.spatial_type,
        ":",
        $.heading_name,
        choice(
          seq($._indent, repeat($._body_item), $._dedent),
          $._newline,
        ),
      ),

    heading_marker: (_$) => token(prec(10, /#{1,4}/)),

    spatial_type: (_$) =>
      token(prec(5, choice("Site", "Building", "Storey", "Space",
        "AssetLibrary", "Asset", "Part"))),

    heading_name: (_$) => /[^\n]+/,

    // --- Element declarations ---
    element_declaration: ($) =>
      seq(
        $.element_type,
        ":",
        optional($.element_id),
        choice(
          seq($._indent, repeat($._body_item), $._dedent),
          $._newline,
        ),
      ),

    element_type: (_$) =>
      token(
        prec(
          5,
          choice(
            "Wall",
            "Slab",
            "Column",
            "Beam",
            "Roof",
            "Stair",
            "Ramp",
            "Railing",
            "Door",
            "Window",
            "Opening",
            "Furniture",
            "Elevator",
            "Proxy",
            "Member",
            "Plate",
            "Grid",
            "Covering",
            "Footing",
            "Duct",
            "Pipe",
            "CableTray",
            "Terminal",
            "Equipment",
            "Accessory",
            "System",
          ),
        ),
      ),

    element_id: (_$) => /[A-Za-z{][A-Za-z0-9_{}-]*/,

    // --- CheckSuite declaration (free-text name) ---
    check_suite_declaration: ($) =>
      seq(
        token(prec(5, "CheckSuite")),
        ":",
        $.check_suite_name,
        choice(
          seq($._indent, repeat($._check_body_item), $._dedent),
          $._newline,
        ),
      ),

    check_suite_name: (_$) => /[^\n]+/,

    // --- Check declaration ---
    check_declaration: ($) =>
      seq(
        token(prec(5, "Check")),
        ":",
        $.element_id,
        choice(
          seq($._indent, repeat($._body_item), $._dedent),
          $._newline,
        ),
      ),

    // --- Material definitions ---
    material_definition: ($) =>
      seq(
        "Material",
        ":",
        $.material_id,
        choice(
          seq($._indent, repeat($._body_item), $._dedent),
          $._newline,
        ),
      ),

    material_id: (_$) => /[A-Za-z][A-Za-z0-9_-]*/,

    // --- Template definitions ---
    // Template: Name followed by an indented body of properties and elements.
    // Space headings (#### Space:) within the template are parsed as top-level
    // headings and associated with the template during CST-to-AST conversion.
    template_definition: ($) =>
      seq(
        token(prec(6, "Template")),
        ":",
        $.template_id,
        choice(
          seq($._indent, repeat($._body_item), $._dedent),
          $._newline,
        ),
      ),

    template_id: (_$) => /[A-Za-z][A-Za-z0-9_]*/,

    // --- Import statements ---
    import_statement: ($) =>
      seq(
        "import",
        field("name", $.identifier),
        "from",
        field("path", $.string),
        $._newline,
      ),

    // --- Body items (inside indented blocks) ---
    _body_item: ($) =>
      choice(
        $.property,
        $.element_declaration,
        $.check_declaration,
        $.comment,
        $.doc_comment,
        $._newline,
      ),

    // Check suite body items (checks + properties)
    _check_body_item: ($) =>
      choice(
        $.property,
        $.check_declaration,
        $.comment,
        $.doc_comment,
        $._newline,
      ),

    // --- Properties ---
    property: ($) => choice($.simple_property, $.list_property),

    // Indented list of `- key: value` items, e.g. the layers of a material:
    //   layers:
    //     - gypsum_board: 0.0125 m
    //     - mineral_wool: 0.075 m
    list_property: ($) =>
      seq(
        field("key", $.property_key),
        ":",
        $._indent,
        repeat1(choice($.list_item, $.comment, $._newline)),
        $._dedent,
      ),

    list_item: ($) =>
      seq(
        "-",
        field("key", choice($.element_ref, $.identifier)),
        ":",
        field("value", $._value),
        $._newline,
      ),

    simple_property: ($) =>
      seq(
        field("key", $.property_key),
        ":",
        field("value", $._value),
        $._newline,
      ),

    property_key: (_$) => /[a-z][a-z0-9_]*/,

    // --- Values ---
    _value: ($) =>
      choice(
        $.axis_value,
        $.geometry_primitive,
        $.number_pair,
        $.number_with_unit,
        $.number,
        $.string,
        $.boolean,
        $.point3d,
        $.point,
        $.polygon_with_holes,
        $.polygon,
        $.profile,
        $.path_expression,
        $.element_ref,
        $.identifier,
        $.enum_list,
        $.number_list,
        // Check expression values (lower priority — tried after simpler alternatives)
        $.comparison_expression,
        $.in_expression,
      ),

    // --- Check expression values ---

    // Comparison: operand op operand (e.g., width >= 0.90 m)
    comparison_expression: ($) =>
      prec.left(1, seq(
        field("left", $._check_operand),
        $.comparison_op,
        field("right", $._check_operand),
      )),

    // In expression: operand in [list] (e.g., usage in [living, bedroom])
    in_expression: ($) =>
      prec(2, seq(
        field("left", $._check_operand),
        "in",
        field("right", $.enum_list),
      )),

    _check_operand: ($) =>
      choice(
        $.number_with_unit,
        $.number,
        $.boolean,
        $.string,
        $.function_call,
        $.path_expression,
        $.identifier,
      ),

    // Dotted path: host.space.usage
    path_expression: ($) =>
      prec(4, seq(
        $.identifier,
        repeat1(seq(".", $.identifier)),
      )),

    // Comparison operators
    comparison_op: (_$) =>
      token(choice(">=", "<=", "==", "!=", ">", "<")),

    // Function call: count(), sum(x), min(x)
    function_call: ($) =>
      seq(
        field("name", $.identifier),
        "(",
        optional(commaSep1($._check_operand)),
        ")",
      ),

    // Axis value: (x1, y1) to: (x2, y2) or (x1, y1) via: (x, y) to: (x2, y2).
    // Endpoints may carry a z height above the storey: (x1, y1, z1) to: (x2, y2, z2)
    axis_value: ($) =>
      seq(
        field("from", choice($.point3d, $.point)),
        optional(seq("via", ":", field("via", $.point))),
        "to",
        ":",
        field("to", choice($.point3d, $.point)),
      ),

    // Bare comma-separated number pair: 49.4521, 12.1803
    number_pair: ($) =>
      prec(3, seq($.number, ",", $.number)),

    number: (_$) => token(prec(1, /[-]?\d+(\.\d+)?/)),

    number_with_unit: ($) =>
      prec(2, seq($.number, $.unit)),

    unit: (_$) =>
      token(
        prec(
          3,
          choice(
            "m",
            "mm",
            "cm",
            "km",
            "m2",
            "m3",
            "ft",
            "inch",
            "°",
            "deg",
            "rad",
            "W/m2K",
            "kg/m3",
            "dB",
            "kN",
            "kN/m",
            "kN/m2",
            "MPa",
          ),
        ),
      ),

    string: (_$) => /"([^"\\]|\\.)*"/,

    boolean: (_$) => token(choice("true", "false")),

    // 3D point: (x, y, z)
    point3d: ($) =>
      prec(4, seq("(", $.number, ",", $.number, ",", $.number, ")")),

    point: ($) =>
      seq("(", $.number, ",", $.number, ")"),

    // Point list: corner points, `smooth (x, y)` through-points and `nurbs { ... }` segments.
    // 2D lists are outlines, 3D lists are routes (e.g. of a duct); cst_to_ast tells them
    // apart by dimension and rejects lists that mix both.
    polygon: ($) =>
      seq("[", commaSep1($._path_item), "]"),

    _path_item: ($) => choice($.point3d, $.point, $.smooth_point, $.nurbs),

    smooth_point: ($) => seq("smooth", choice($.point3d, $.point)),

    // Exact NURBS segment from its first to its last control point
    nurbs: ($) =>
      seq(
        "nurbs", "{",
        "degree", ":", field("degree", $.number),
        ",", "points", ":", "[", commaSep1(choice($.point3d, $.point)), "]",
        optional(seq(",", "weights", ":", field("weights", $.number_array))),
        optional(seq(",", "knots", ":", field("knots", $.number_array))),
        "}",
      ),

    number_array: ($) =>
      seq("[", commaSep1($.number), "]"),

    // Polygon with holes: { outer: [...], holes: [[...], [...]] }
    polygon_with_holes: ($) =>
      seq(
        "{",
        "outer",
        ":",
        field("outer", $.polygon),
        optional(seq(",", "holes", ":", "[", field("holes", commaSep1($.polygon)), "]")),
        "}",
      ),

    // Geometry primitive for asset parts: box(w,d,h), cylinder(r,h), extrude([...], h), import("path")
    geometry_primitive: ($) =>
      prec(5, seq(
        $.geometry_type,
        "(",
        optional($._geometry_arg_list),
        ")",
      )),

    geometry_type: (_$) =>
      token(prec(6, choice("box", "cylinder", "extrude"))),

    _geometry_arg_list: ($) =>
      commaSep1($._geometry_arg),

    _geometry_arg: ($) =>
      choice(
        $.number,
        $.string,
        $.polygon,
      ),

    profile: ($) =>
      seq(
        $.profile_type,
        "(",
        commaSep1($.number),
        ")",
      ),

    profile_type: (_$) =>
      token(choice("rectangle", "circle", "i_profile")),

    // Identifiers or hyphenated element ids (e.g. `under: [W-01, W-02]`).
    enum_list: ($) =>
      seq("[", commaSep1(choice($.element_ref, $.identifier)), "]"),

    // Lengths (e.g. `transoms: [1.1 m, 3.4 m]`).
    number_list: ($) =>
      seq("[", commaSep1(choice($.number_with_unit, $.number)), "]"),

    identifier: (_$) => /[a-zA-Z][a-zA-Z0-9_]*/,

    // Reference to a hyphenated element id (e.g. `opening: OP-GF-01`).
    element_ref: (_$) => /[a-zA-Z][a-zA-Z0-9_]*-[a-zA-Z0-9_-]*/,

    // --- Comments ---
    comment: ($) =>
      seq(token(prec(10, "//")), /[^\n]*/, $._newline),

    doc_comment: ($) =>
      seq(token(prec(10, "///")), /[^\n]*/, $._newline),
  },
});

function commaSep1(rule) {
  return seq(rule, repeat(seq(",", rule)));
}
