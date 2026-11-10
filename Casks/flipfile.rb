cask "flipfile" do
  arch arm: "aarch64", intel: "x64"

  version "0.7.1"
  sha256 arm:   "02037f2c89fd8d8bb5293bee2efe9b784f258c7ac1eae77f434e480dce8a3833",
         intel: "36c70a9d014e1ba06c5a6ffcb8591d6a16a05313d4b97fa0f2d29d8a8ed99f9b"

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
