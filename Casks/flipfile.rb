cask "flipfile" do
  arch arm: "aarch64", intel: "x64"

  version "0.9.0"
  sha256 arm:   "6ea81ed4c0bf85dc4a3864f594c8fc48fb1fc2f274d90b84fba27f1d2575d136",
         intel: "9b5870bc2f3abbd7cdd4adaf0af3a2e989f6bef1ec6481b817b1af2ab0d98d59"

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
