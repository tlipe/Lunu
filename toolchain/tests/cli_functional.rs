use std::process::Command;
use tempfile::tempdir;

#[test]
fn cli_init_creates_project_layout() {
    let dir = tempdir().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_lunu"))
        .arg("init")
        .env("LUNU_INIT_RUNTIME", "lune")
        .current_dir(dir.path())
        .status()
        .unwrap();
    assert!(status.success());
    assert!(dir.path().join("modules").join("lunu").join("init.luau").exists());
    assert!(dir.path().join("lunu.toml").exists());
    assert!(dir.path().join(".luaurc").exists());
}

#[test]
fn cli_check_runs_on_initialized_project() {
    let dir = tempdir().unwrap();
    let status = Command::new(env!("CARGO_BIN_EXE_lunu"))
        .arg("init")
        .env("LUNU_INIT_RUNTIME", "lune")
        .current_dir(dir.path())
        .status()
        .unwrap();
    assert!(status.success());

    let status = Command::new(env!("CARGO_BIN_EXE_lunu"))
        .arg("check")
        .current_dir(dir.path())
        .status()
        .unwrap();
    assert!(status.success());
}
