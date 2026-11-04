//! Links libmanette 0.2 from the GNOME runtime, which ships it (0.2.13 in GNOME 51).

fn main() {
    if std::env::var("DOCS_RS").is_ok() {
        return;
    }
    println!("cargo:rustc-link-lib=manette-0.2");
}
