fn main() {
    println!("cargo:rerun-if-changed=cpp/turbo_engine.cpp");
    println!("cargo:rerun-if-changed=cpp/include/turbo_engine.h");
    cc::Build::new()
        .cpp(true)
        .file("cpp/turbo_engine.cpp")
        .include("cpp/include")
        .flag("-std=c++11")
        .flag("-fPIC")
        .compile("turbo_engine");
    println!("cargo:rustc-link-search=native=/usr/lib/x86_64-linux-gnu");
    println!("cargo:rustc-link-search=native=/lib/x86_64-linux-gnu");
    println!("cargo:rustc-link-lib=dylib=curl");
    println!("cargo:rustc-link-lib=dylib=ssl");
    println!("cargo:rustc-link-lib=dylib=crypto");
    println!("cargo:rustc-link-lib=dylib=stdc++");
}
//Черт(тут должно быть другое слово) что я написал вообще