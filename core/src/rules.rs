//! Indexing rules. One struct, stored as JSON in the settings table, applied in
//! both the sweep and the watcher so a rule means the same thing everywhere.
//! The UI shows these as switches; the presets expand to concrete names here.

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Rules {
    /// Skip folders that hold build output, dependencies and version control.
    pub skip_dev_folders: bool,
    /// Skip hidden files and folders (dot-prefixed, and AppData on Windows).
    pub skip_hidden: bool,
    /// Skip installers, archives and disk images.
    pub skip_packages: bool,
    /// Skip video and audio files.
    pub skip_media: bool,
    /// Skip whole code projects: folders with version control or a build file
    /// (Makefile, package.json, ...). A folder added as a source is never skipped
    /// this way, so adding a project on purpose still indexes it.
    pub skip_code_projects: bool,
    /// Skip pictures named only by a camera or a number (IMG_0412.jpg,
    /// Screenshot 2026-01-02.png): a picture is found by its name, and these
    /// names say nothing. See `camera_name`.
    pub skip_textless_images: bool,
    /// Skip files larger than this many MB. 0 = no limit.
    pub max_mb: u64,
    /// Follow shortcuts and linked folders.
    pub follow_links: bool,
    /// Folders the user picked to skip, absolute paths.
    pub skipped_folders: Vec<String>,
    /// Only index these kinds. Empty = all. Values match item kinds:
    /// chat is not a file kind, so: pdf, document, spreadsheet, code, image, text.
    pub only_kinds: Vec<String>,
}

impl Default for Rules {
    fn default() -> Self {
        Self {
            skip_dev_folders: true,
            skip_hidden: true,
            skip_packages: true,
            skip_media: true,
            skip_code_projects: true,
            skip_textless_images: true,
            max_mb: 64,
            follow_links: false,
            skipped_folders: vec![],
            only_kinds: vec![],
        }
    }
}

const DEV_FOLDERS: &[&str] = &["node_modules", "target", ".git", "dist", "build", "__pycache__", "venv", ".venv", ".cache", "obj", "bin", ".next", ".nuxt", "vendor"];
const HIDDEN_FOLDERS: &[&str] = &["AppData", "Library"];
const PACKAGE_EXT: &[&str] = &["exe", "msi", "dmg", "pkg", "appx", "iso", "img", "zip", "7z", "rar", "gz", "tgz", "bz2", "xz", "jar", "apk"];
/// Files that mark a folder as a code project.
const PROJECT_MARKERS: &[&str] = &[".git", "Makefile", "makefile", "CMakeLists.txt", "meson.build", "package.json", "Cargo.toml", "pyproject.toml", "setup.py", "go.mod", "pom.xml", "build.gradle", "composer.json", "Gemfile"];
const MEDIA_EXT: &[&str] = &["mp4", "mkv", "mov", "avi", "webm", "m4v", "mp3", "wav", "flac", "m4a", "aac", "ogg"];

/// Compiled form; build once per sweep.
pub struct Matcher {
    rules: Rules,
    folders: GlobSet,
}

fn is_code_project(dir: &Path) -> bool {
    dir.is_dir() && PROJECT_MARKERS.iter().any(|m| dir.join(m).exists())
}

impl Matcher {
    pub fn new(rules: Rules) -> Self {
        let mut b = GlobSetBuilder::new();
        let mut names: Vec<String> = vec![];
        if rules.skip_dev_folders {
            names.extend(DEV_FOLDERS.iter().map(|s| s.to_string()));
        }
        if rules.skip_hidden {
            names.extend(HIDDEN_FOLDERS.iter().map(|s| s.to_string()));
        }
        for n in names {
            for pat in [format!("**/{n}"), format!("**/{n}/**")] {
                if let Ok(g) = Glob::new(&pat) {
                    b.add(g);
                }
            }
        }
        for f in &rules.skipped_folders {
            let f = f.trim().replace('\\', "/");
            if f.is_empty() {
                continue;
            }
            for pat in [f.clone(), format!("{f}/**")] {
                if let Ok(g) = Glob::new(&pat) {
                    b.add(g);
                }
            }
        }
        Self { rules, folders: b.build().unwrap_or_else(|_| GlobSet::empty()) }
    }

    pub fn rules(&self) -> &Rules {
        &self.rules
    }

    /// Should the walker descend into / consider this entry at all.
    pub fn allows_entry(&self, path: &Path, depth: usize) -> bool {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if self.rules.skip_hidden && name.starts_with('.') && depth > 0 {
            return false;
        }
        if self.rules.skip_code_projects && depth > 0 && is_code_project(path) {
            return false;
        }
        let norm = path.to_string_lossy().replace('\\', "/");
        !self.folders.is_match(norm.as_str())
    }

    /// Whether a file sits inside a code project below `root`, for files that
    /// arrive one at a time (the folder watcher) rather than through the walk.
    pub fn in_code_project(&self, file: &Path, root: &Path) -> bool {
        self.rules.skip_code_projects
            && file.ancestors().skip(1).take_while(|a| *a != root && a.starts_with(root)).any(is_code_project)
    }

    /// Should this file be indexed. `kind` is what the extractor would call it.
    pub fn allows_file(&self, path: &Path, size: u64, kind: &str) -> bool {
        if self.rules.max_mb > 0 && size > self.rules.max_mb * 1024 * 1024 {
            return false;
        }
        if !self.allows_entry(path, 1) {
            return false;
        }
        let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
        if self.rules.skip_packages && PACKAGE_EXT.contains(&ext.as_str()) {
            return false;
        }
        if self.rules.skip_media && MEDIA_EXT.contains(&ext.as_str()) {
            return false;
        }
        if !self.rules.only_kinds.is_empty() && !self.rules.only_kinds.iter().any(|k| k == kind) {
            return false;
        }
        true
    }
}

/// True when a picture's file name says nothing about it: once camera and
/// screenshot prefixes and digits are taken out, fewer than three letters are
/// left (`IMG_0412`, `DSC01234`, `Screenshot 2026-01-02 101010`, `a1`).
pub fn camera_name(stem: &str) -> bool {
    // longest first, so "whatsapp image" goes before "image" and "dscn" before "dsc"
    const PREFIXES: &[&str] = &[
        "whatsapp image", "screen shot", "screenshot", "telegram", "picsart", "capture", "edited",
        "signal", "mvimg", "image", "photo", "dscn", "dscf", "scan", "img", "dsc", "pxl", "pic",
    ];
    let mut name = stem.to_lowercase().replace(['_', '-', '.', '(', ')'], " ");
    for p in PREFIXES {
        name = name.replace(p, " ");
    }
    let letters = name.chars().filter(|c| c.is_alphabetic()).count();
    letters < 3
}

#[cfg(test)]
mod tests {
    #[test]
    fn camera_names_say_nothing_and_real_names_do() {
        for n in ["IMG_0412", "DSC01234", "PXL_20260102_101010123", "Screenshot 2026-01-02 101010", "WhatsApp Image 2026-01-02 at 10.10.10", "a1", "0001"] {
            assert!(camera_name(n), "{n} should count as a camera name");
        }
        for n in ["vent_diagram", "kitchen floor plan", "IMG_0412 passport", "receipt", "Screenshot of the error"] {
            assert!(!camera_name(n), "{n} should count as a real name");
        }
    }

    use super::*;

    #[test]
    fn code_projects_skipped_but_never_the_source_itself() {
        let root = std::env::temp_dir().join(format!("recall-rules-{}", std::process::id()));
        let game = root.join("Games").join("emerald").join("emerald"); // nested, as unzipped downloads are
        let notes = root.join("Notes");
        std::fs::create_dir_all(game.join("src")).unwrap();
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::write(game.join("Makefile"), "all:").unwrap();
        std::fs::write(game.join("src").join("main.c"), "int main(){}").unwrap();

        // the temp folder sits under AppData, which the hidden-folders rule skips
        let m = Matcher::new(Rules { skip_hidden: false, ..Default::default() });
        assert!(!m.allows_entry(&game, 3), "a folder with a Makefile is skipped by the walk");
        assert!(m.allows_entry(&root.join("Games").join("emerald"), 2));
        assert!(m.allows_entry(&notes, 1));
        assert!(m.in_code_project(&game.join("src").join("main.c"), &root), "a watched file inside it is caught too");
        assert!(!m.in_code_project(&game.join("src").join("main.c"), &game), "a project added as a source is indexed");
        assert!(m.allows_entry(&game, 0), "depth 0 (the source) is never skipped");
        let off = Matcher::new(Rules { skip_code_projects: false, skip_hidden: false, ..Default::default() });
        assert!(off.allows_entry(&game, 3));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dev_folders_skipped_when_on() {
        let m = Matcher::new(Rules::default());
        assert!(!m.allows_entry(Path::new("/x/y/node_modules"), 3));
        assert!(m.allows_entry(Path::new("/x/y/src"), 3));
        let m = Matcher::new(Rules { skip_dev_folders: false, ..Default::default() });
        assert!(m.allows_entry(Path::new("/x/y/node_modules"), 3));
    }

    #[test]
    fn picked_folder_skipped() {
        let m = Matcher::new(Rules { skipped_folders: vec!["C:/Users/me/Downloads/installers".into()], ..Default::default() });
        assert!(!m.allows_entry(Path::new("C:\\Users\\me\\Downloads\\installers\\setup"), 4));
        assert!(m.allows_entry(Path::new("C:\\Users\\me\\Downloads\\papers"), 3));
    }

    #[test]
    fn only_kinds_and_size() {
        let m = Matcher::new(Rules { only_kinds: vec!["pdf".into()], ..Default::default() });
        assert!(m.allows_file(Path::new("/a/b.pdf"), 10, "pdf"));
        assert!(!m.allows_file(Path::new("/a/b.md"), 10, "text"));
        let m = Matcher::new(Rules::default());
        assert!(!m.allows_file(Path::new("/a/big.md"), 65 * 1024 * 1024, "text"));
        assert!(!m.allows_file(Path::new("/a/setup.exe"), 10, "text"));
    }
}
