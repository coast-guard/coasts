# Homebrew formula for Coast — Containerized Host.
#
# This is a template. The actual formula in the coast-guard/tap tap
# is generated automatically by the release workflow with correct URLs and
# SHA256 checksums. This file serves as documentation and a starting point
# for the tap repo.
#
# Users install via:
#   brew tap coast-guard/coasts
#   brew install coast
class Coast < Formula
  desc "Containerized Host — isolated dev environments on a single machine"
  homepage "https://github.com/coast-guard/coasts"
  version "0.1.0"
  license "MIT"

  depends_on "socat"

  on_macos do
    on_arm do
      url "https://github.com/coast-guard/coasts/releases/download/v0.1.0/coast-v0.1.0-darwin-arm64.tar.gz"
      sha256 "PLACEHOLDER"
    end
    on_intel do
      url "https://github.com/coast-guard/coasts/releases/download/v0.1.0/coast-v0.1.0-darwin-amd64.tar.gz"
      sha256 "PLACEHOLDER"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/coast-guard/coasts/releases/download/v0.1.0/coast-v0.1.0-linux-arm64.tar.gz"
      sha256 "PLACEHOLDER"
    end
    on_intel do
      url "https://github.com/coast-guard/coasts/releases/download/v0.1.0/coast-v0.1.0-linux-amd64.tar.gz"
      sha256 "PLACEHOLDER"
    end
  end

  def install
    bin.install "coast"
    bin.install "coastd"
  end

  service do
    run [opt_bin/"coastd", "--foreground"]
    keep_alive true
    log_path var/"log/coastd.log"
    error_log_path var/"log/coastd.error.log"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/coast --version")
  end
end
