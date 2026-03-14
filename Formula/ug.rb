class Ug < Formula
  desc "UniGateway – lightweight LLM gateway with OpenAI/Anthropic compatibility"
  homepage "https://github.com/EeroEternal/unigateway"
  version "0.3.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/EeroEternal/unigateway/releases/download/v#{version}/ug-aarch64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER"
    end
    on_intel do
      url "https://github.com/EeroEternal/unigateway/releases/download/v#{version}/ug-x86_64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/EeroEternal/unigateway/releases/download/v#{version}/ug-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "PLACEHOLDER"
    end
  end

  def install
    bin.install "ug"
  end

  test do
    assert_match "ug", shell_output("#{bin}/ug --version")
  end
end
