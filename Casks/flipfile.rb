cask "flipfile" do
  arch arm: "aarch64", intel: "x64"

  version "0.12.0"
  sha256 arm:   "0ce230f37d5a2c42a35c574d867c61df3b5cd865454d04863924b5288d39a34a",
         intel: "a58e7d5a07bc481cb306b7aabc87f7648f9caad3f72d86d76aa0cdd1a1eabe41"

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
