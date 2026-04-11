fn main() {
    // Re-run this build script (and recompile the crate) whenever web assets change.
    println!("cargo:rerun-if-changed=web/dist");
}
