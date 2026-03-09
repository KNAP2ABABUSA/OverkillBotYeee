fn main() {
    println!("cargo:rerun-if-changed=cpp/turbo_engine.cpp");
    println!("cargo:rerun-if-changed=cpp/include/turbo_engine.h");
    println!("cargo:warning=Build script started");
    println!("cargo:rustc-link-search=native=/usr/lib/x86_64-linux-gnu");
    println!("cargo:rustc-link-search=native=/lib/x86_64-linux-gnu");
    println!("cargo:rustc-link-arg=-lcurl");
    println!("cargo:rustc-link-arg=-lssl");
    println!("cargo:rustc-link-arg=-lcrypto");
    println!("cargo:warning=pkg-config libcurl found");
    cc::Build::new()
        .cpp(true)
        .file("cpp/turbo_engine.cpp")
        .include("cpp/include")
        .include("/usr/include/x86_64-linux-gnu")
        .flag("-std=c++11")
        .flag("-fPIC")
        .compile("turbo_engine");
    println!("cargo:warning=Compilation finished");}
