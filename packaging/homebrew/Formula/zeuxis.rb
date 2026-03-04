class Zeuxis < Formula
  desc "Local read-only MCP screenshot server"
  homepage "https://github.com/bnomei/zeuxis"
  version "0.1.0"
  license "MIT"

  checksums = {
    aarch64_apple_darwin: "REPLACE_WITH_SHA256",
    x86_64_apple_darwin: "REPLACE_WITH_SHA256",
    aarch64_unknown_linux_musl: "REPLACE_WITH_SHA256",
    x86_64_unknown_linux_musl: "REPLACE_WITH_SHA256",
  }

  on_macos do
    on_arm do
      url "https://github.com/bnomei/zeuxis/releases/download/v#{version}/zeuxis-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 checksums[:aarch64_apple_darwin]
    end
    on_intel do
      url "https://github.com/bnomei/zeuxis/releases/download/v#{version}/zeuxis-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 checksums[:x86_64_apple_darwin]
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/bnomei/zeuxis/releases/download/v#{version}/zeuxis-v#{version}-aarch64-unknown-linux-musl.tar.gz"
      sha256 checksums[:aarch64_unknown_linux_musl]
    end
    on_intel do
      url "https://github.com/bnomei/zeuxis/releases/download/v#{version}/zeuxis-v#{version}-x86_64-unknown-linux-musl.tar.gz"
      sha256 checksums[:x86_64_unknown_linux_musl]
    end
  end

  def install
    bin.install "zeuxis"
  end

  test do
    assert_match "zeuxis", shell_output("#{bin}/zeuxis --help")
  end
end
