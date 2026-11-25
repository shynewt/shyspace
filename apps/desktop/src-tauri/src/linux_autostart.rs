//! Opening at login on Linux: an XDG autostart entry, a `.desktop` file in
//! `$XDG_CONFIG_HOME/autostart` (`~/.config/autostart` by default). The file
//! is the system's record, so it's read each time: one the desktop turned
//! off (`Hidden=true`, or GNOME's `X-GNOME-Autostart-enabled=false`) counts
//! as off.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const FILE_NAME: &str = "dev.shynewt.shyspace.desktop";

/// The autostart folder: `$XDG_CONFIG_HOME/autostart`, or
/// `~/.config/autostart` when that's unset, empty or relative (the spec
/// says to ignore a relative one).
pub fn autostart_dir(xdg_config_home: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    let config = xdg_config_home
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| home.map(|home| home.join(".config")))?;
    Some(config.join("autostart"))
}

/// shyspace's autostart entry in a folder.
pub struct Autostart {
    file: PathBuf,
}

impl Autostart {
    pub fn in_dir(dir: &Path) -> Autostart {
        Autostart {
            file: dir.join(FILE_NAME),
        }
    }

    /// Whether the desktop will start shyspace at login.
    pub fn is_on(&self) -> Result<bool> {
        match std::fs::read_to_string(&self.file) {
            Ok(contents) => Ok(is_on(&contents)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error).with_context(|| format!("reading {}", self.file.display())),
        }
    }

    /// Writes the entry for `program` (replacing one turned off elsewhere),
    /// unless it's already exactly that.
    pub fn turn_on(&self, program: &Path, argument: &str) -> Result<()> {
        let wanted = entry(program, argument)?;
        if std::fs::read_to_string(&self.file).is_ok_and(|current| current == wanted) {
            return Ok(());
        }
        let dir = self.file.parent().context("the autostart entry has no folder")?;
        std::fs::create_dir_all(dir).with_context(|| format!("making {}", dir.display()))?;
        // Written beside it, then moved over it, so it's never half written.
        let written = self.file.with_extension("desktop.new");
        std::fs::write(&written, wanted).with_context(|| format!("writing {}", written.display()))?;
        std::fs::rename(&written, &self.file).with_context(|| format!("writing {}", self.file.display()))
    }

    pub fn turn_off(&self) -> Result<()> {
        match std::fs::remove_file(&self.file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                Err(error).with_context(|| format!("removing {}", self.file.display()))
            }
            _ => Ok(()),
        }
    }
}

/// The `.desktop` file that starts `program` with `argument`.
fn entry(program: &Path, argument: &str) -> Result<String> {
    let program = program.to_str().context("the app's path isn't valid text")?;
    Ok(format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=shyspace\n\
         Comment=Opens shyspace in the background when you log in\n\
         Exec={} {argument}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n",
        exec_argument(program)?
    ))
}

/// `text` as one argument of an `Exec` key, per the Desktop Entry spec:
/// quoted, with `"`, `` ` ``, `$` and `\` escaped inside the quotes, `%`
/// doubled (it starts a field code), and then the string escaping every
/// value takes, which doubles each backslash again.
fn exec_argument(text: &str) -> Result<String> {
    if text.chars().any(char::is_control) {
        bail!("the app's path has a control character, which an autostart entry can't hold");
    }
    let mut quoted = String::from("\"");
    for character in text.chars() {
        match character {
            '"' | '`' | '$' => {
                quoted.push('\\');
                quoted.push(character);
            }
            '\\' => quoted.push_str("\\\\"),
            '%' => quoted.push_str("%%"),
            _ => quoted.push(character),
        }
    }
    quoted.push('"');
    Ok(quoted.replace('\\', "\\\\"))
}

/// Whether an autostart entry's `[Desktop Entry]` group leaves it on.
fn is_on(contents: &str) -> bool {
    let mut is_in_entry = false;
    for line in contents.lines().map(str::trim) {
        if line.starts_with('[') {
            is_in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !is_in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match (key.trim(), value.trim()) {
            ("Hidden", "true") | ("X-GNOME-Autostart-enabled", "false") => return false,
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_follows_xdg_config_home_when_its_absolute() {
        let home = Some(PathBuf::from("/home/ada"));
        assert_eq!(
            autostart_dir(Some("/data/config".into()), home.clone()),
            Some(PathBuf::from("/data/config/autostart"))
        );
        for ignored in [None, Some(OsString::new()), Some("relative/config".into())] {
            assert_eq!(
                autostart_dir(ignored.clone(), home.clone()),
                Some(PathBuf::from("/home/ada/.config/autostart")),
                "{ignored:?}"
            );
        }
        assert_eq!(autostart_dir(None, None), None);
    }

    #[test]
    fn the_path_is_quoted_and_escaped_as_one_argument() {
        assert_eq!(exec_argument("/opt/shyspace").unwrap(), r#""/opt/shyspace""#);
        assert_eq!(
            exec_argument("/home/ada/My Apps/shyspace.AppImage").unwrap(),
            r#""/home/ada/My Apps/shyspace.AppImage""#
        );
        // Each of these must reach the program as the character itself.
        assert_eq!(exec_argument(r#"/a"b"#).unwrap(), r#""/a\\"b""#);
        assert_eq!(exec_argument("/a$b`c").unwrap(), r#""/a\\$b\\`c""#);
        assert_eq!(exec_argument(r"/a\b").unwrap(), r#""/a\\\\b""#);
        assert_eq!(exec_argument("/100%/x").unwrap(), r#""/100%%/x""#);
    }

    #[test]
    fn a_path_with_a_line_break_is_refused() {
        assert!(exec_argument("/a\nExec=/bin/evil").is_err());
        assert!(entry(Path::new("/a\rb"), "--autostart").is_err());
    }

    #[test]
    fn an_entry_turned_off_by_the_desktop_counts_as_off() {
        let on = entry(Path::new("/opt/shyspace"), "--autostart").unwrap();
        assert!(is_on(&on));
        assert!(!is_on(&format!("{on}Hidden=true\n")));
        assert!(!is_on(&on.replace(
            "X-GNOME-Autostart-enabled=true",
            "X-GNOME-Autostart-enabled=false"
        )));
        assert!(
            is_on(&format!("{on}[Desktop Action other]\nHidden=true\n")),
            "only the main group counts"
        );
        assert!(is_on(&format!("{on}# Hidden=true\n")));
    }

    #[test]
    fn turning_on_writes_the_entry_again_for_a_moved_app_and_off_removes_it() {
        let dir = std::env::temp_dir().join(format!("shyspace-autostart-test-{}", std::process::id()));
        let autostart = Autostart::in_dir(&dir.join("autostart"));
        assert!(!autostart.is_on().unwrap());

        autostart
            .turn_on(Path::new("/old/shyspace.AppImage"), "--autostart")
            .unwrap();
        assert!(autostart.is_on().unwrap());
        autostart
            .turn_on(Path::new("/new/shyspace.AppImage"), "--autostart")
            .unwrap();
        let written = std::fs::read_to_string(dir.join("autostart").join(FILE_NAME)).unwrap();
        assert!(
            written.contains(r#"Exec="/new/shyspace.AppImage" --autostart"#),
            "{written}"
        );

        autostart.turn_off().unwrap();
        assert!(!autostart.is_on().unwrap());
        autostart.turn_off().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
