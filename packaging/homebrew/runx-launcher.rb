class Runx < Formula
  desc "Run any project with the exact runtime versions it declares"
  homepage "https://github.com/aryankahar31/runx"
  version "__VERSION__"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/aryankahar31/runx/releases/download/v#{version}/runx-macos-arm64.tar.gz"
      sha256 "__SHA256_macos_arm64__"
    else
      url "https://github.com/aryankahar31/runx/releases/download/v#{version}/runx-macos-x64.tar.gz"
      sha256 "__SHA256_macos_x64__"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/aryankahar31/runx/releases/download/v#{version}/runx-linux-arm64.tar.gz"
      sha256 "__SHA256_linux_arm64__"
    else
      url "https://github.com/aryankahar31/runx/releases/download/v#{version}/runx-linux-x64.tar.gz"
      sha256 "__SHA256_linux_x64__"
    end
  end

  def install
    bin.install "runx"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/runx --version")
  end
end
