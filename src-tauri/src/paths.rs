use std::path::PathBuf;

pub(crate) fn home() -> Option<PathBuf> {
    // Windows 上 `HOME` 可能是 Git Bash 之类设的 POSIX 路径（`/c/Users/...`），
    // 这里不认；所以那边优先 `USERPROFILE`，macOS / Linux 反过来优先 `HOME`。
    let order: [&str; 2] = if cfg!(windows) {
        ["USERPROFILE", "HOME"]
    } else {
        ["HOME", "USERPROFILE"]
    };
    order
        .into_iter()
        .find_map(std::env::var_os)
        .map(PathBuf::from)
}
