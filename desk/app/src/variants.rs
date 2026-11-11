//! Internal asset keys keep normal and shiny pictures/models separate; API dex numbers stay unchanged.
const SHINY: u64 = 1 << 32;
pub fn key(number: u64, shiny: bool) -> u64 {
    number | if shiny { SHINY } else { 0 }
}
pub fn number(key: u64) -> u64 {
    key & !SHINY
}
pub fn shiny(key: u64) -> bool {
    key & SHINY != 0
}
pub fn of(mon: &serde_json::Value) -> Option<u64> {
    Some(key(mon["dex"].as_u64()?, mon["shiny"] == true))
}
pub fn folder(key: u64) -> String {
    format!(
        "pm{:04}_00_00{}",
        number(key),
        if shiny(key) { "_rare" } else { "" }
    )
}
pub fn picture(root: &std::path::Path, key: u64) -> std::path::PathBuf {
    let root = if shiny(key) {
        root.join("shiny")
    } else {
        root.to_path_buf()
    };
    root.join(format!("{}.png", number(key)))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn variants_never_change_national_numbers_or_collide() {
        assert_eq!(folder(key(6, true)), "pm0006_00_00_rare");
        assert_eq!(folder(key(6, false)), "pm0006_00_00");
        assert_eq!(number(key(1025, true)), 1025);
        assert_ne!(key(6, true), key(6, false));
        assert_eq!(
            picture(std::path::Path::new("icons"), key(6, true)),
            std::path::Path::new("icons/shiny/6.png")
        );
    }
}
