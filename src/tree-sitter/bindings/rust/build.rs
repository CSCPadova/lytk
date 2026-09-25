fn main() {
    let root_dir = std::path::Path::new("src").join("tree-sitter");
    let lilypond_dir = root_dir.join("src");
    let lilypond_scheme_dir = root_dir.join("lilypond-scheme").join("src");

    let mut c_config = cc::Build::new();
    // Say which C: older compilers (the manylinux2014 aarch64 GCC) default to
    // C89, which rejects the generated parser's `for (int i = …)` loops.
    c_config.std("c11").include(&lilypond_dir);
    c_config
        .flag_if_supported("-Wno-unused-parameter")
        .flag_if_supported("-Wno-unused-but-set-variable")
        .flag_if_supported("-Wno-trigraphs");
    #[cfg(target_env = "msvc")]
    c_config.flag("-utf-8");

    for path in &[
        lilypond_dir.join("parser.c"),
        lilypond_scheme_dir.join("parser.c"),
    ] {
        c_config.file(path);
        println!("cargo:rerun-if-changed={}", path.to_str().unwrap());
    }

    c_config.compile("tree-sitter-lilypond");
}
