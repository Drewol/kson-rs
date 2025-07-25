use anyhow::anyhow;
use log::info;
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};
//TODO: Move to platform files
#[cfg(all(target_os = "windows", not(feature = "portable")))]
pub fn default_game_dir() -> PathBuf {
    let mut game_dir = directories::UserDirs::new()
        .expect("Failed to get directories")
        .document_dir()
        .expect("Failed to get documents directory")
        .to_path_buf();
    game_dir.push("USC");
    game_dir
}

#[cfg(all(target_os = "windows", feature = "portable"))]
pub fn default_game_dir() -> PathBuf {
    let mut game_dir = std::env::current_exe().expect("Could not get exe path");
    game_dir.pop();
    game_dir
}

#[cfg(not(target_os = "windows"))]
pub fn default_game_dir() -> PathBuf {
    if let Some(p) = GAME_DIR_OVERRIDE.get().cloned() {
        p
    } else {
        let mut game_dir = directories::UserDirs::new()
            .expect("Failed to get directories")
            .home_dir()
            .to_path_buf();
        game_dir.push(".local");
        game_dir.push("share");
        game_dir.push("usc");
        game_dir
    }
}

pub static GAME_DIR_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();
pub static INSTALL_DIR_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

#[cfg(target_os = "android")]
pub fn init_game_dir(game_dir: impl AsRef<Path>) -> anyhow::Result<()> {
    use include_dir::*;
    static SKIN_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/skins");
    static FONT_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/fonts");

    let mut p = game_dir.as_ref().to_path_buf();
    p.push("skins");
    std::fs::create_dir_all(&p)?;
    SKIN_DIR.extract(p)?;

    let mut p = game_dir.as_ref().to_path_buf();
    p.push("fonts");
    std::fs::create_dir_all(&p)?;
    FONT_DIR.extract(p)?;
    Ok(())
}

fn is_install_dir(dir: impl AsRef<Path>) -> Option<PathBuf> {
    let dir = dir.as_ref();
    let font_dir = dir.join("fonts");
    let skin_dir = dir.join("skins");
    if font_dir.exists() && skin_dir.exists() {
        Some(dir.to_path_buf())
    } else {
        None
    }
}

#[cfg(not(target_os = "android"))]
pub fn init_game_dir(game_dir: impl AsRef<Path>) -> anyhow::Result<()> {
    #[cfg(feature = "portable")]
    {
        return Ok(());
    }

    let mut candidates = vec![];

    let cargo_dir = std::env::var("CARGO_MANIFEST_DIR");

    if let Ok(dir) = &cargo_dir {
        candidates.push(PathBuf::from(dir));
    }

    candidates.push(std::env::current_dir()?);

    let mut candidate_dir = std::env::current_exe()?;
    candidate_dir.pop();
    candidates.push(candidate_dir.clone());
    #[cfg(target_os = "macos")]
    {
        //if app bundle
        if candidate_dir.with_file_name("Resources").exists() {
            candidate_dir.set_file_name("Resources");
        }
    }
    #[cfg(target_os = "linux")]
    {
        //deb installs files to usr/lib/rusc/game
        // assume starting at usr/bin after popping exe
        candidate_dir.pop(); // usr
        candidate_dir.push("lib");
        candidate_dir.push("rusc");
        candidate_dir.push("game");
    }

    candidates.push(candidate_dir);

    let install_dir = candidates
        .iter()
        .filter_map(is_install_dir)
        .next()
        .ok_or(anyhow!("Failed to find installed files"))?;

    if install_dir.as_path() == game_dir.as_ref() {
        info!("Running from install dir");
        return Ok(());
    }

    std::fs::create_dir_all(&game_dir)?;

    let r = install_dir.read_dir()?;
    for ele in r.into_iter() {
        let ele = ele?;
        let folder_name = ele
            .file_name()
            .into_string()
            .map_err(|_| anyhow!("Bad file name"))?;

        if ele.file_type()?.is_dir() && (folder_name == "fonts" || folder_name == "skins") {
            // Quickly check if the root path exists, ignore it if it does
            let path = ele.path();
            let target = path.strip_prefix(&install_dir)?;
            let mut target_path = game_dir.as_ref().to_path_buf();
            target_path.push(target);

            // Always install when cargo in cargo for easier skin dev
            if target_path.exists() && cargo_dir.is_err() {
                continue;
            }

            for data_file in walkdir::WalkDir::new(path).into_iter() {
                let data_file = data_file?;

                let target_file = data_file.path().strip_prefix(&install_dir)?;
                let mut target_path = game_dir.as_ref().to_path_buf();
                target_path.push(target_file);

                if data_file.file_type().is_dir() {
                    std::fs::create_dir_all(target_path)?;
                    continue;
                }

                info!("Installing: {:?} -> {:?}", data_file.path(), &target_path);
                std::fs::copy(data_file.path(), target_path)?;
            }
        }
    }

    Ok(())
}

pub fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "Drewol", "USC")
}
