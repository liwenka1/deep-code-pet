fn main() {
    tauri_build::build();

    // `generate_context!` 会在编译期把图标字节嵌进二进制，但它不给 icons 目录发
    // `cargo:rerun-if-changed`（见 tauri-build 里 CodegenContext 的注释：只有 crate
    // 重新编译才会重新生成）。结果是"换了 icon、重启 dev，Dock 还是旧图标"。
    //
    // 这里把图标内容的哈希作为 rustc-env 暴露出去：内容一变，env 变，cargo 就会
    // 重新编译本 crate，`generate_context!` 随之重新读图标。
    println!("cargo:rerun-if-changed=icons");
    let icon = std::fs::read("icons/icon.icns").unwrap_or_default();
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a 64
    for byte in &icon {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    println!("cargo:rustc-env=PET_ICON_HASH={hash:016x}");
}
