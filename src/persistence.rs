use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::{PROJECT_FORMAT_VERSION, Project};

pub fn default_autosave_path() -> PathBuf {
    let base = env::var_os("LOCALAPPDATA")
        .or_else(|| env::var_os("APPDATA"))
        .map(PathBuf::from)
        .unwrap_or_else(env::temp_dir);
    base.join("KLAUS Sequencer 58")
        .join("autosave.klaus58.json")
}

pub fn load_project(path: &Path) -> Result<Project, String> {
    let json = fs::read_to_string(path)
        .map_err(|error| format!("Projekt konnte nicht gelesen werden: {error}"))?;
    let mut project =
        Project::from_json(&json).map_err(|error| format!("Projektdatei ist ungültig: {error}"))?;
    project.ensure_lane_coverage();
    project.ensure_patterns();
    project.format_version = PROJECT_FORMAT_VERSION;
    project.validate_chains()?;
    Ok(project)
}

pub fn save_project_atomic(project: &Project, path: &Path) -> Result<(), String> {
    let json = project
        .to_json()
        .map_err(|error| format!("Projekt konnte nicht serialisiert werden: {error}"))?;
    let parent = path
        .parent()
        .ok_or_else(|| "Autosave-Pfad hat kein übergeordnetes Verzeichnis".to_owned())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Autosave-Verzeichnis konnte nicht erstellt werden: {error}"))?;

    let temporary = sibling_with_suffix(path, ".tmp");
    let backup = sibling_with_suffix(path, ".bak");
    fs::write(&temporary, json)
        .map_err(|error| format!("Temporärer Autosave konnte nicht geschrieben werden: {error}"))?;

    if path.exists() {
        remove_if_exists(&backup).map_err(|error| {
            format!("Alte Autosave-Sicherung konnte nicht entfernt werden: {error}")
        })?;
        fs::rename(path, &backup)
            .map_err(|error| format!("Autosave konnte nicht gesichert werden: {error}"))?;
    }

    if let Err(error) = fs::rename(&temporary, path) {
        if backup.exists() {
            let _ = fs::rename(&backup, path);
        }
        return Err(format!("Autosave konnte nicht aktiviert werden: {error}"));
    }
    let _ = remove_if_exists(&backup);
    Ok(())
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map_or_else(|| "autosave".into(), |value| value.to_os_string());
    name.push(suffix);
    path.with_file_name(name)
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{load_project, save_project_atomic};
    use crate::model::Project;

    #[test]
    fn atomic_save_round_trip_preserves_project() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("klaus58-test-{unique}.json"));
        let project = Project::demo();
        save_project_atomic(&project, &path).expect("save project");
        let restored = load_project(&path).expect("load project");
        assert_eq!(restored, project);
        let _ = fs::remove_file(path);
    }
}
