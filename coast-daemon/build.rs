use std::path::Path;
use std::process::Command;

fn main() {
    let guard_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../coast-guard");

    if !guard_dir.join("package.json").exists() {
        println!("cargo:warning=coast-guard not found, skipping UI build");
        return;
    }

    // Re-run if coast-guard sources change
    println!("cargo:rerun-if-changed=../coast-guard/src");
    println!("cargo:rerun-if-changed=../coast-guard/index.html");
    println!("cargo:rerun-if-changed=../coast-guard/package.json");
    println!("cargo:rerun-if-changed=../coast-guard/vite.config.ts");
    println!("cargo:rerun-if-changed=../coast-guard/tsconfig.json");
    println!("cargo:rerun-if-changed=../coast-guard/tailwind.config.ts");
    println!("cargo:rerun-if-changed=../docs");
    println!("cargo:rerun-if-changed=../search-indexes");

    // Skip the npm build in CI or when explicitly opted out
    if std::env::var("COAST_SKIP_UI_BUILD").is_ok() {
        println!("cargo:warning=COAST_SKIP_UI_BUILD set, skipping UI build");
        return;
    }

    let dist_index = guard_dir.join("dist").join("index.html");
    if dist_index.exists() {
        // Already built — don't rebuild unless sources changed (cargo handles
        // this via rerun-if-changed directives above).
        return;
    }

    // Install deps if needed
    if !guard_dir.join("node_modules").exists() {
        let status = Command::new("npm")
            .arg("install")
            .current_dir(&guard_dir)
            .status()
            .expect("failed to run npm install for coast-guard");
        assert!(status.success(), "npm install failed for coast-guard");
    }

    let status = Command::new("npm")
        .args(["run", "build"])
        .current_dir(&guard_dir)
        .status()
        .expect("failed to run npm build for coast-guard");
    assert!(status.success(), "coast-guard UI build failed");
}
