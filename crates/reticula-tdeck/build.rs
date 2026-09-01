use embuild::espidf;

fn main() {
    // Links ESP-IDF components so cargo knows about them at build time.
    espidf::sysenv().unwrap();
    println!("cargo:rustc-cfg=esp_idf");
}