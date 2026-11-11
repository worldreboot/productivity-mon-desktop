//! Prepare a normal/shiny HOME variant from the repository's local source bundles.
#![allow(dead_code)]
#[path = "../src/bake.rs"]
mod bake;
#[path = "../src/variants.rs"]
mod variants;
fn main() {
    let models = std::path::PathBuf::from(std::env::args().nth(1).expect("models folder"));
    let selection = std::env::args().nth(2).expect("dex number or all-shiny");
    let shiny = std::env::args().nth(3).as_deref() == Some("shiny");
    std::fs::create_dir_all(&models).expect("models folder");
    let baker = bake::Baker::find(&models).expect("models inside the source repository");
    let keys: Vec<u64> = if selection == "all-shiny" {
        std::fs::read_dir(&models)
            .unwrap()
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                let dex = name
                    .strip_prefix("pm")?
                    .strip_suffix("_00_00")?
                    .parse::<u64>()
                    .ok()?;
                entry
                    .path()
                    .join("model.mon")
                    .is_file()
                    .then_some(variants::key(dex, true))
            })
            .collect()
    } else {
        vec![variants::key(selection.parse().expect("dex number"), shiny)]
    };
    for key in keys {
        let folder = models.join(variants::folder(key));
        if !folder.join("model.mon").is_file() {
            baker.bake(key).expect("prepare HOME model");
        }
        println!("Prepared {}", folder.display());
    }
}
