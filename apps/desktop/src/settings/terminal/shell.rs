use crate::tr;
use gpui_kit::{SharedString, component::searchable_list::SearchableListItem};

#[derive(Clone)]
pub(in crate::settings) struct Shell(pub String);

impl Shell {
    pub(super) fn items(paths: &[String], selected: &str) -> Vec<Self> {
        let mut items = vec![Self(String::new())];
        items.extend(paths.iter().cloned().map(Self));
        if !items.iter().any(|item| item.0 == selected) {
            items.push(Self(selected.to_owned()));
        }
        items
    }
}

impl SearchableListItem for Shell {
    type Value = String;

    fn title(&self) -> SharedString {
        if self.0.is_empty() {
            tr("terminal_shell_default")
        } else if let Some(key) = name(&self.0) {
            rust_i18n::t!("terminal_shell_option", name = tr(key), path = &self.0).into()
        } else {
            self.0.clone().into()
        }
    }

    fn value(&self) -> &String {
        &self.0
    }
}

fn name(path: &str) -> Option<&'static str> {
    let executable = path.rsplit(['/', '\\']).next()?.to_ascii_lowercase();
    let executable = executable.strip_suffix(".exe").unwrap_or(&executable);
    Some(match executable {
        "cmd" => "terminal_shell_cmd",
        "powershell" => "terminal_shell_windows_powershell",
        "pwsh" => "terminal_shell_powershell",
        "bash"
            if path.split(['/', '\\']).any(|part| {
                ["git", "gitforwindows", "portablegit"]
                    .iter()
                    .any(|name| part.eq_ignore_ascii_case(name))
            }) =>
        {
            "terminal_shell_git_bash"
        }
        "bash" => "terminal_shell_bash",
        "zsh" => "terminal_shell_zsh",
        "fish" => "terminal_shell_fish",
        "nu" => "terminal_shell_nu",
        "sh" => "terminal_shell_sh",
        "csh" => "terminal_shell_csh",
        "tcsh" => "terminal_shell_tcsh",
        "ksh" => "terminal_shell_ksh",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_execution_host_paths() {
        for (path, expected) in [
            (r"C:\Windows\System32\CMD.EXE", "terminal_shell_cmd"),
            (
                r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
                "terminal_shell_windows_powershell",
            ),
            (
                r"C:\Program Files\PowerShell\7\pwsh.exe",
                "terminal_shell_powershell",
            ),
            (
                r"D:\Tools\PortableGit\bin\bash.exe",
                "terminal_shell_git_bash",
            ),
            (
                "C:/Program Files/Git/bin/bash.exe",
                "terminal_shell_git_bash",
            ),
            ("/bin/zsh", "terminal_shell_zsh"),
            ("/opt/homebrew/bin/bash", "terminal_shell_bash"),
            ("/usr/local/bin/fish", "terminal_shell_fish"),
        ] {
            assert_eq!(name(path), Some(expected), "{path}");
        }
        for path in ["/Applications/iTerm.app", r"C:\Windows\System32\wsl.exe"] {
            assert_eq!(name(path), None);
        }
    }

    #[test]
    fn preserves_default_and_configured_paths() {
        let paths = vec!["/bin/zsh".to_owned()];
        let selected = r"D:\Custom Shell\shell.exe";
        let items = Shell::items(&paths, selected);
        assert_eq!(items[0].value(), "");
        assert_eq!(items[1].value(), "/bin/zsh");
        assert_eq!(items[2].value(), selected);
        assert_eq!(Shell::items(&paths, "/bin/zsh").len(), 2);
        assert_eq!(Shell::items(&paths, "").len(), 2);
    }
}
