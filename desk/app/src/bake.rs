//! Baking a species' model the first time it is needed, not all thousand beforehand: the app runs
//! desk/bake/bake.ts on the HOME model the viewer has cached, into its models folder. Only where the app is run
//! from a source repository. Missing normal/shiny viewer caches are converted from the local HOME bundles.

#[cfg(target_os="windows")]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};

pub struct Baker {
    repo: PathBuf,
    models: PathBuf,
    converting: std::sync::Mutex<()>,
}

/// The folder a species' base form is baked into, and cached under by the viewer, by its Pokédex number.
pub fn folder(dex: u64) -> String {
    crate::variants::folder(dex)
}

impl Baker {
    /// A baker for `models`, if that is `desk/app/models` of a repository with the bake script and the tool that
    /// runs it.
    pub fn find(models: &Path) -> Option<Baker> {
        let models = std::fs::canonicalize(models).ok()?;
        let repo = models
            .ancestors()
            .find(|path| path.join("desk/bake/bake.ts").is_file())?
            .to_path_buf();
        let here = repo.join("desk/bake/bake.ts").exists()
            && repo
                .join(tsx_path())
                .exists();
        here.then_some(Baker {
            repo,
            models,
            converting: std::sync::Mutex::new(()),
        })
    }

    /// Where the viewer keeps a species' model, as it gave it.
    fn source(&self, dex: u64) -> PathBuf {
        self.repo
            .join("tools/home-viewer/.cache")
            .join(format!("v7-{}", folder(dex)))
    }

    /// Whether there is a model of this species to bake.
    pub fn can(&self, dex: u64) -> bool {
        self.source(dex).join("model.json").exists() || self.bundle(dex).is_some()
    }

    fn bundle(&self, dex: u64) -> Option<PathBuf> {
        let root = self.repo.parent()?.join("PokemonHOMEBundles");
        let filename = format!("{}.unity3d", folder(dex));
        std::fs::read_dir(root)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("pokemons").join(&filename))
            .find(|path| path.is_file())
    }

    fn convert(&self, dex: u64) -> Result<(), String> {
        let _lock = self
            .converting
            .lock()
            .map_err(|_| "model conversion lock".to_string())?;
        if self.source(dex).join("model.json").is_file() {
            return Ok(());
        }
        let bundle = self.bundle(dex).ok_or("HOME variant unavailable")?;
        let tool = self.repo.join("tools/home-viewer");
        let mut command = std::process::Command::new(tool.join(if cfg!(target_os="windows") {".venv/Scripts/python.exe"} else {".venv/bin/python3"}));
        quiet(&mut command);
        let output = command
            .arg(tool.join("convert.py"))
            .arg("--repo")
            .arg(self.repo.parent().unwrap().join("PokemonHOMEBundles"))
            .arg("--source")
            .arg(bundle)
            .arg("--output")
            .arg(self.source(dex))
            .arg("--cache")
            .arg(tool.join(".cache"))
            .output()
            .map_err(|error| error.to_string())?;
        if output.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&output.stderr)
                .lines()
                .last()
                .unwrap_or("model conversion failed")
                .to_string())
        }
    }

    /// Bakes a species, taking a second or so. Into a folder beside the one it will have and then renamed, so
    /// that a half-made model is never taken for a whole one.
    pub fn bake(&self, dex: u64) -> Result<(), String> {
        self.convert(dex)?;
        let (wip, done) = (
            self.models.join(format!("{}.baking", folder(dex))),
            self.models.join(folder(dex)),
        );
        if !wip.starts_with(&self.models) || !done.starts_with(&self.models) {
            return Err("model output escaped its root".into());
        }
        let _ = std::fs::remove_dir_all(&wip);
        // Windows' own long form of a path (\\?\C:\...) is not for a command line.
        let plain = |path: &Path| {
            path.display()
                .to_string()
                .trim_start_matches(r"\\?\")
                .to_string()
        };
        #[cfg(target_os="windows")]
        let mut command = { let mut command=std::process::Command::new("cmd"); command.arg("/C").arg(plain(&self.repo.join(tsx_path()))); command };
        #[cfg(not(target_os="windows"))]
        let mut command = std::process::Command::new(self.repo.join(tsx_path()));
        quiet(&mut command);
        let ran = command
            .arg(plain(&self.repo.join("desk/bake/bake.ts")))
            .arg(plain(&self.source(dex)))
            .arg(plain(&wip))
            .current_dir(plain(&self.repo))
            // No console window flashing up over the desktop.
            .output()
            .map_err(|e| e.to_string())?;
        if !ran.status.success() || !wip.join("model.mon").exists() {
            let _ = std::fs::remove_dir_all(&wip);
            return Err(String::from_utf8_lossy(&ran.stderr)
                .lines()
                .last()
                .unwrap_or("the bake made nothing")
                .to_string());
        }
        let _ = std::fs::remove_dir_all(&done);
        std::fs::rename(&wip, &done).map_err(|e| e.to_string())
    }
}

fn tsx_path() -> &'static str {
    if cfg!(target_os="windows") {"packages/client/node_modules/.bin/tsx.cmd"} else {"packages/client/node_modules/.bin/tsx"}
}
fn quiet(command: &mut std::process::Command) {
    #[cfg(target_os="windows")]
    {command.creation_flags(0x0800_0000);}
    #[cfg(not(target_os="windows"))]
    let _=command;
}
