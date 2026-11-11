//! Reuse the shared server classifications for common apps on both operating systems.
const APPS: &[(&str, &str)] = &[
    ("com.microsoft.VSCode", "code.exe"),
    ("com.microsoft.VSCodeInsiders", "code - insiders.exe"),
    ("com.todesktop.230313mzl4w4u92", "cursor.exe"),
    ("com.exafunction.windsurf", "windsurf.exe"),
    ("com.google.Chrome", "chrome.exe"),
    ("com.microsoft.edgemac", "msedge.exe"),
    ("org.mozilla.firefox", "firefox.exe"),
    ("com.brave.Browser", "brave.exe"),
    ("com.operasoftware.Opera", "opera.exe"),
    ("com.vivaldi.Vivaldi", "vivaldi.exe"),
    ("company.thebrowser.Browser", "arc.exe"),
    ("app.zen-browser.zen", "zen.exe"),
    ("org.chromium.Chromium", "chromium.exe"),
    ("com.apple.Safari", "safari"),
    ("com.apple.Terminal", "terminal"),
    ("com.googlecode.iterm2", "iterm2"),
    ("com.microsoft.Word", "winword.exe"),
    ("com.microsoft.Excel", "excel.exe"),
    ("com.microsoft.Powerpoint", "powerpnt.exe"),
    ("com.microsoft.Outlook", "outlook.exe"),
    ("com.microsoft.onenote.mac", "onenote.exe"),
    ("com.microsoft.teams2", "ms-teams.exe"),
    ("com.hnc.Discord", "discord.exe"),
    ("com.tinyspeck.slackmacgap", "slack.exe"),
    ("com.spotify.client", "spotify.exe"),
    ("notion.id", "notion.exe"),
    ("com.figma.Desktop", "figma.exe"),
    ("md.obsidian", "obsidian.exe"),
    ("com.valvesoftware.steam", "steam.exe"),
    ("org.blenderfoundation.blender", "blender.exe"),
    ("com.openai.chat", "chatgpt.exe"),
    ("com.openai.codex", "codex.exe"),
    ("com.anthropic.claudefordesktop", "claude.exe"),
    ("com.apple.finder", "finder"),
    ("com.apple.dt.Xcode", "xcode"),
    ("com.apple.TextEdit", "textedit"),
];

pub(super) fn app_key(bundle: &str) -> String {
    APPS.iter()
        .find(|(id, _)| id.eq_ignore_ascii_case(bundle))
        .map_or_else(|| bundle.to_lowercase(), |(_, key)| (*key).to_string())
}
pub(super) fn bundle_id(key: &str) -> Option<&str> {
    APPS.iter()
        .find(|(_, id)| *id == key)
        .map(|(bundle, _)| *bundle)
        .or_else(|| (key.contains('.') && !key.ends_with(".exe")).then_some(key))
}

#[cfg(test)]
mod tests {
    #[test]
    fn mac_apps_share_common_classifications_without_merging_unknown_apps() {
        assert_eq!(super::app_key("com.microsoft.VSCode"), "code.exe");
        assert_eq!(super::app_key("COM.GOOGLE.CHROME"), "chrome.exe");
        assert_eq!(super::app_key("com.apple.Safari"), "safari");
        assert_eq!(super::app_key("org.example.Editor"), "org.example.editor");
        assert_eq!(super::bundle_id("code.exe"), Some("com.microsoft.VSCode"));
        assert_eq!(
            super::bundle_id("org.example.editor"),
            Some("org.example.editor")
        );
        assert_eq!(super::bundle_id("unknown.exe"), None);
    }
}
