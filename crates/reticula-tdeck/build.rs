use embuild::espidf::sysenv;

fn main() {
    // Links ESP-IDF components so cargo knows about them at build time.
    sysenv::output();
    println!("cargo:rustc-cfg=esp_idf");
}