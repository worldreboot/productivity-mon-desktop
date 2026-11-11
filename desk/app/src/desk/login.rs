use std::path::Path;

fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub(super) fn launch_agent(executable: &Path, models: &Path, server: &str) -> String {
    let args = [
        executable.to_string_lossy().into_owned(),
        "--models".into(),
        models.to_string_lossy().into_owned(),
        "--server".into(),
        server.into(),
    ];
    let args = args
        .iter()
        .map(|arg| format!("<string>{}</string>", xml(arg)))
        .collect::<String>();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict><key>Label</key><string>com.productivitymon.desktop</string>
<key>ProgramArguments</key><array>{args}</array><key>RunAtLoad</key><true/>
<key>ProcessType</key><string>Interactive</string></dict></plist>
"#
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn login_arguments_are_separate_xml_values_and_never_shell_commands() {
        let plist = super::launch_agent(
            std::path::Path::new("/Apps/ProductivityMon & Co/desk-mons"),
            std::path::Path::new("/Models/<alpha>"),
            "https://example.com/?a=1&b=2",
        );
        assert!(plist.contains("ProductivityMon &amp; Co"));
        assert!(plist.contains("&lt;alpha&gt;"));
        assert!(plist.contains("?a=1&amp;b=2"));
        assert!(plist.contains("<string>--models</string><string>"));
        assert!(!plist.contains("/bin/sh"));
    }
}
