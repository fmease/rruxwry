use std::{fs, sync::LazyLock};

fn has_docker_env_file() -> bool {
    fs::metadata("/.dockerenv").is_ok()
}

fn has_docker_in_cgroup() -> bool {
    fs::read_to_string("/proc/self/cgroup").is_ok_and(|cgroup| cgroup.contains("docker"))
}

pub fn is_docker() -> bool {
    static IS_DOCKER: LazyLock<bool> =
        LazyLock::new(|| has_docker_env_file() || has_docker_in_cgroup());
    *IS_DOCKER
}
