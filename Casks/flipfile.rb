cask "flipfile" do
  arch arm: "aarch64", intel: "x64"

  version "0.9.1"
  sha256 arm:   "02317a4a559139a528e055026c9abd723da5bb730934d74ae366cea1e3530126",
         intel: "bf9a62d0db3ad4f992af5415603bb6cda2007cd180c86989546b799ca1a1c4fb"

  url "https://github.com/wcksnrdn/flipfile/releases/download/v#{version}/flipfile_#{version}_#{arch}.dmg"
  name "flipfile"
  desc "Tiny offline radial desktop file converter for photos, PDFs, Office, video and audio"
  homepage "https://github.com/wcksnrdn/flipfile"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: ">= :ventura"

  app "flipfile.app"

  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-cr", "#{appdir}/flipfile.app"],
                   sudo: false
  end

  zap trash: [
    "~/Library/Application Support/com.satria.flipfile",
    "~/Library/Caches/com.satria.flipfile",
    "~/Library/Preferences/com.satria.flipfile.plist",
    "~/Library/Saved Application State/com.satria.flipfile.savedState",
  ]
end
