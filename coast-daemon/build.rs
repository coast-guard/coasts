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

    // If the UI is already built (e.g. by a prior CI step), skip the npm build.
    if guard_dir.join("dist/index.html").exists() {
        println!("cargo:warning=coast-guard/dist/index.html already exists, skipping UI build");
        return;
    }

    // Skip the npm build in CI or when explicitly opted out.
    // Create a stub dist/ so rust-embed compiles even without a real UI build.
    if std::env::var("COAST_SKIP_UI_BUILD").is_ok() {
        println!("cargo:warning=COAST_SKIP_UI_BUILD set, skipping UI build");
        let dist_dir = guard_dir.join("dist");
        if !dist_dir.join("index.html").exists() {
            std::fs::create_dir_all(&dist_dir).expect("failed to create stub dist/");
            std::fs::write(dist_dir.join("index.html"), "<!-- stub: UI not built -->")
                .expect("failed to write stub index.html");
        }
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
