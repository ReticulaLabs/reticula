use embuild::espidf;

fn main() {
    espidf::sysenv().unwrap();
    println!("cargo:rustc-cfg=esp_idf");
}