cask "flipfile" do
  arch arm: "aarch64", intel: "x64"

  version "0.3.0"
  sha256 :no_check

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
