cask "flipfile" do
  arch arm: "aarch64", intel: "x64"

  version "0.6.1"
  sha256 arm:   "95cad59da269590d201f51d37d677151120e5b06164add577b10e66c61d188cb",
         intel: "9031b8b26a2408da1d86a3d673e4536efa1e42c8d3c080d97b00c03ccbf33802"

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
