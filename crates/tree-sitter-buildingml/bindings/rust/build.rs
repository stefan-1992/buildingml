fn main() {
    let src_dir = std::path::Path::new("src");

    println!("cargo:rerun-if-changed=src/parser.c");
    println!("cargo:rerun-if-changed=src/scanner.c");

    let mut c_config = cc::Build::new();
    c_config.std("c11");
    c_config.include(src_dir);

    // Main parser
    c_config.file(src_dir.join("parser.c"));

    // External scanner
    let scanner_path = src_dir.join("scanner.c");
    if scanner_path.exists() {
        c_config.file(&scanner_path);
    }

    c_config.compile("tree-sitter-buildingml");
}
