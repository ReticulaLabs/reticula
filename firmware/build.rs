use embuild::espidf::sysenv;

fn main() {
    sysenv::output();
    println!("cargo:rustc-cfg=esp_idf");
}