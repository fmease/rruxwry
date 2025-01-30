use std::{env, fs, io, sync::LazyLock};

pub fn is_wsl() -> bool {
    static IS_WSL: LazyLock<bool> = LazyLock::new(|| {
        if env::consts::OS != "linux" {
            return false;
        }

        if let Ok(os_release) = get_os_release() {
            if os_release.to_lowercase().contains("microsoft") {
                return !is_docker::is_docker();
            }
        }

        if proc_version_includes_microsoft() { !is_docker::is_docker() } else { false }
    });
    *IS_WSL
}

fn proc_version_includes_microsoft() -> bool {
    fs::read_to_string("/proc/version")
        .is_ok_and(|version| version.to_lowercase().contains("microsoft"))
}

// This function is copied from the sys-info crate to avoid taking a dependency on all of sys-info
// https://docs.rs/sys-info/0.9.1/src/sys_info/lib.rs.html#426-433
//
// The MIT License (MIT)
//
// Copyright (c) 2015 Siyu Wang
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
fn get_os_release() -> io::Result<String> {
    let mut release = fs::read_to_string("/proc/sys/kernel/osrelease")?;
    release.pop(); // pop '\n'
    Ok(release)
}
