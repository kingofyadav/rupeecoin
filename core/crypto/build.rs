fn main() {
    cc::Build::new()
        .file("c/dilithium3_seeded.c")
        .opt_level(2)
        .compile("dilithium3_seeded");
    // Re-run if the C source changes.
    println!("cargo:rerun-if-changed=c/dilithium3_seeded.c");
}
