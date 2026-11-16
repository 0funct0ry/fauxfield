cask "fauxfield" do
  version "__VERSION__"
  sha256 "__DMG_SHA256__"

  url "https://github.com/0funct0ry/fauxfield/releases/download/v#{version}/Fauxfield-#{version}-macos-universal.dmg"
  name "Fauxfield"
  desc "Local-first mock API tool"
  homepage "https://0funct0ry.github.io/fauxfield/"

  depends_on macos: ">= :monterey"

  app "Fauxfield.app"
  binary "#{appdir}/Fauxfield.app/Contents/MacOS/fauxfield"
end
