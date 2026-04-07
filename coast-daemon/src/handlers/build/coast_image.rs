use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tracing::info;

use coast_core::coastfile::Coastfile;
use coast_core::error::{CoastError, Result};
use coast_core::protocol::BuildProgressEvent;

use super::emit;
use super::plan::BuildPlan;
use super::utils::shell_single_quote;

pub(super) async fn build_coast_image(
    coastfile: &Coastfile,
    build_id: &str,
    progress: &tokio::sync::mpsc::Sender<BuildProgressEvent>,
    plan: &BuildPlan,
) -> Result<Option<String>> {
    if coastfile.setup.is_empty() {
        return Ok(None);
    }

    let image_tag = format!("coast-image/{}:{}", coastfile.name, build_id);
    emit(progress, plan.started("Building coast image"));
    info!(image = %image_tag, "building custom coast image from [coast.setup]");

    let dockerfile = render_dockerfile(coastfile);
    let dockerfile_hash = sha256_hex(&dockerfile);
    let no_cache = has_dockerfile_changed(&coastfile.name, &dockerfile_hash);

    if no_cache {
        info!("coast image Dockerfile template changed, rebuilding without cache");
    }

    let build_dir = prepare_build_dir(coastfile, &dockerfile)?;
    let build_output = run_docker_build(&image_tag, build_dir.path(), no_cache).await?;

    if !build_output.status.success() {
        let stderr = String::from_utf8_lossy(&build_output.stderr);
        emit(
            progress,
            BuildProgressEvent::done("Building coast image", "fail")
                .with_verbose(stderr.to_string()),
        );
        return Err(CoastError::docker(format!(
            "Failed to build custom coast image '{}'. \
             Check that the packages, commands, and files in [coast.setup] are valid.\n\
             Stderr: {stderr}",
            image_tag
        )));
    }

    save_dockerfile_hash(&coastfile.name, &dockerfile_hash);

    emit(
        progress,
        BuildProgressEvent::done("Building coast image", "ok")
            .with_verbose(String::from_utf8_lossy(&build_output.stdout).to_string()),
    );
    info!(image = %image_tag, "custom coast image built successfully");

    let latest_tag = format!("coast-image/{}:latest", coastfile.name);
    tag_latest_image(&image_tag, &latest_tag).await;

    Ok(Some(image_tag))
}

fn sha256_hex(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn dockerfile_hash_path(project: &str) -> PathBuf {
    let coast_home = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".coast");
    coast_home.join("coast-image-dockerfile-hash").join(project)
}

fn has_dockerfile_changed(project: &str, current_hash: &str) -> bool {
    let path = dockerfile_hash_path(project);
    match std::fs::read_to_string(&path) {
        Ok(stored) => stored.trim() != current_hash,
        Err(_) => true,
    }
}

fn save_dockerfile_hash(project: &str, hash: &str) {
    let path = dockerfile_hash_path(project);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, hash);
}

fn render_dockerfile(coastfile: &Coastfile) -> String {
    let mut dockerfile = String::from("FROM docker:dind\n");
    dockerfile.push_str("RUN apk add --no-cache ripgrep fd rsync\n");
    dockerfile.push_str(
        "RUN ARCH=$(uname -m) && case \"$ARCH\" in x86_64) MA=amd64;; aarch64|arm64) MA=arm64;; esac \
         && wget -q -O /tmp/mutagen.tar.gz \
         \"https://github.com/mutagen-io/mutagen/releases/download/v0.18.1/mutagen_linux_${MA}_v0.18.1.tar.gz\" \
         && tar xzf /tmp/mutagen.tar.gz -C /usr/local/bin mutagen mutagen-agents.tar.gz \
         && chmod +x /usr/local/bin/mutagen && rm -f /tmp/mutagen.tar.gz\n",
    );
    if !coastfile.setup.packages.is_empty() {
        dockerfile.push_str(&format!(
            "RUN apk add --no-cache {}\n",
            coastfile.setup.packages.join(" ")
        ));
    }
    dockerfile.push_str(
        "RUN command -v npm >/dev/null 2>&1 && \
         npm install -g typescript-language-server typescript \
         vscode-langservers-extracted yaml-language-server \
         pyright 2>/dev/null || true\n",
    );
    for command in &coastfile.setup.run {
        dockerfile.push_str(&format!("RUN {}\n", command));
    }
    if !coastfile.setup.files.is_empty() {
        dockerfile.push_str("COPY setup-files/ /\n");
        for file in &coastfile.setup.files {
            if let Some(mode) = file.mode.as_deref() {
                dockerfile.push_str(&format!(
                    "RUN chmod {} {}\n",
                    mode,
                    shell_single_quote(&file.path)
                ));
            }
        }
    }
    dockerfile
}

fn prepare_build_dir(coastfile: &Coastfile, dockerfile: &str) -> Result<tempfile::TempDir> {
    let build_dir = tempfile::tempdir().map_err(|error| {
        CoastError::io_simple(format!(
            "failed to create temp dir for coast image build: {error}"
        ))
    })?;
    write_setup_files(coastfile, build_dir.path())?;
    let dockerfile_path = build_dir.path().join("Dockerfile");
    std::fs::write(&dockerfile_path, dockerfile).map_err(|error| CoastError::Io {
        message: format!("failed to write coast image Dockerfile: {error}"),
        path: dockerfile_path.clone(),
        source: Some(error),
    })?;
    Ok(build_dir)
}

fn write_setup_files(coastfile: &Coastfile, build_dir: &Path) -> Result<()> {
    if coastfile.setup.files.is_empty() {
        return Ok(());
    }

    let setup_root = build_dir.join("setup-files");
    for file in &coastfile.setup.files {
        let rel = file.path.trim_start_matches('/');
        let rel_path = Path::new(rel);
        let out_path = setup_root.join(rel_path);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| CoastError::Io {
                message: format!("failed to create setup file parent '{}': {error}", rel),
                path: parent.to_path_buf(),
                source: Some(error),
            })?;
        }
        std::fs::write(&out_path, &file.content).map_err(|error| CoastError::Io {
            message: format!("failed to write setup file '{}': {error}", file.path),
            path: out_path.clone(),
            source: Some(error),
        })?;
    }

    Ok(())
}

async fn run_docker_build(
    image_tag: &str,
    build_dir: &Path,
    no_cache: bool,
) -> Result<std::process::Output> {
    let mut cmd = tokio::process::Command::new("docker");
    cmd.arg("build");
    if no_cache {
        cmd.arg("--no-cache");
    }
    cmd.args(["-t", image_tag, build_dir.to_str().unwrap_or(".")]);
    cmd.output().await.map_err(|error| {
        CoastError::docker(format!(
            "failed to run docker build for coast image: {error}. Is Docker running?"
        ))
    })
}

async fn tag_latest_image(image_tag: &str, latest_tag: &str) {
    let _ = tokio::process::Command::new("docker")
        .args(["tag", image_tag, latest_tag])
        .output()
        .await;
}
