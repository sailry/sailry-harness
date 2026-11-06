use crate::tr;
use gpui_kit::{
    component::{searchable_list::SearchableListItem, tooltip::Tooltip},
    prelude::FluentBuilder as _,
    *,
};
use sailry_protocol::terminal;
use std::collections::BTreeMap;

#[derive(Clone)]
pub(in crate::settings) struct Shell {
    value: terminal::Shell,
    variant: Option<String>,
}

impl Shell {
    pub(super) fn items(shells: &[terminal::Shell], selected: &terminal::Shell) -> Vec<Self> {
        let mut values = vec![terminal::Shell::default()];
        for shell in shells {
            if !values.iter().any(|value| equivalent(value, shell)) {
                values.push(shell.clone());
            }
        }
        if let Some(value) = values.iter_mut().find(|value| equivalent(value, selected)) {
            *value = selected.clone();
        } else {
            values.push(selected.clone());
        }

        let mut groups = BTreeMap::<String, Vec<usize>>::new();
        for (index, value) in values.iter().enumerate() {
            groups
                .entry(title(value).to_string())
                .or_default()
                .push(index);
        }
        let mut variants = vec![None; values.len()];
        for group in groups.values().filter(|group| group.len() > 1) {
            for &index in group {
                let peers = group
                    .iter()
                    .filter(|&&peer| peer != index)
                    .map(|&peer| &values[peer])
                    .collect::<Vec<_>>();
                variants[index] = variant(&values[index], &peers);
            }
            let candidates = variants.clone();
            for &index in group {
                let peers = group
                    .iter()
                    .filter(|&&peer| peer != index && candidates[peer] == candidates[index])
                    .map(|&peer| &values[peer])
                    .collect::<Vec<_>>();
                if !peers.is_empty() {
                    variants[index] = installation(&values[index], &peers);
                }
            }
            let candidates = variants.clone();
            for (position, &index) in group.iter().enumerate() {
                if group
                    .iter()
                    .filter(|&&peer| candidates[peer] == candidates[index])
                    .count()
                    > 1
                {
                    variants[index] = Some(
                        rust_i18n::t!("terminal_shell_installation", index = position + 1)
                            .to_string(),
                    );
                }
            }
        }
        values
            .into_iter()
            .zip(variants)
            .map(|(value, variant)| Self { value, variant })
            .collect()
    }
}

impl SearchableListItem for Shell {
    type Value = terminal::Shell;

    fn title(&self) -> SharedString {
        let name = title(&self.value);
        match &self.variant {
            Some(variant) => {
                rust_i18n::t!("terminal_shell_variant", name = name, variant = variant).into()
            }
            None => name,
        }
    }

    fn render(&self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let detail = std::iter::once(self.value.path.as_str())
            .chain(self.value.arguments.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join("\n");
        div()
            .id(SharedString::from(detail.clone()))
            .truncate()
            .child(self.title())
            .when(!self.value.path.is_empty(), |row| {
                row.tooltip(move |window, cx| Tooltip::new(detail.clone()).build(window, cx))
            })
    }

    fn value(&self) -> &terminal::Shell {
        &self.value
    }
}

fn title(shell: &terminal::Shell) -> SharedString {
    if shell.path.is_empty() {
        tr("terminal_shell_default")
    } else if executable(&shell.path).eq_ignore_ascii_case("wsl")
        && let [flag, distribution] = shell.arguments.as_slice()
        && flag == "--distribution"
    {
        rust_i18n::t!("terminal_shell_wsl", name = distribution).into()
    } else if let Some(key) = name(&shell.path) {
        tr(key)
    } else {
        shell
            .path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&shell.path)
            .to_owned()
            .into()
    }
}

fn executable(path: &str) -> String {
    let executable = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    executable
        .strip_suffix(".exe")
        .unwrap_or(&executable)
        .to_owned()
}

fn name(path: &str) -> Option<&'static str> {
    let parts = path
        .split(['/', '\\'])
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    Some(match executable(path).as_str() {
        "cmd" => "terminal_shell_cmd",
        "powershell" => "terminal_shell_windows_powershell",
        "pwsh" => "terminal_shell_powershell",
        "bash"
            if parts.iter().any(|part| {
                matches!(part.as_str(), "git" | "gitforwindows" | "portablegit")
                    || part.starts_with("portablegit-")
            }) =>
        {
            "terminal_shell_git_bash"
        }
        "bash"
            if parts
                .iter()
                .any(|part| matches!(part.as_str(), "msys2" | "msys32" | "msys64")) =>
        {
            "terminal_shell_msys_bash"
        }
        "bash"
            if parts
                .iter()
                .any(|part| matches!(part.as_str(), "cygwin" | "cygwin32" | "cygwin64")) =>
        {
            "terminal_shell_cygwin_bash"
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

fn equivalent(left: &terminal::Shell, right: &terminal::Shell) -> bool {
    identity(&left.path) == identity(&right.path) && left.arguments == right.arguments
}

fn identity(path: &str) -> String {
    if path.starts_with("\\\\") || path.as_bytes().get(1) == Some(&b':') {
        let path = path.replace('\\', "/").to_ascii_lowercase();
        path.strip_prefix("//?/").unwrap_or(&path).to_owned()
    } else {
        path.to_owned()
    }
}

fn features(path: &str) -> Vec<String> {
    let parts = path.split(['/', '\\']).collect::<Vec<_>>();
    let mut features = Vec::new();
    for part in &parts {
        match part.to_ascii_lowercase().as_str() {
            "program files (x86)" | "syswow64" | "x86" | "win-x86" => {
                features.push("x86".to_owned())
            }
            "arm64" | "aarch64" | "win-arm64" => features.push("arm64".to_owned()),
            "x64" | "amd64" | "win-x64" => features.push("x64".to_owned()),
            _ => {}
        }
    }
    if parts.iter().any(|part| {
        let part = part.to_ascii_lowercase();
        part == "preview"
            || part.starts_with("powershell-preview")
            || part.starts_with("pwsh-preview")
            || part.starts_with(|c: char| c.is_ascii_digit()) && part.contains("-preview")
    }) {
        features.push(tr("terminal_shell_preview").to_string());
    }
    if let Some(version) = parts.windows(2).find_map(|parts| {
        (parts[0].eq_ignore_ascii_case("powershell")
            && parts[1].starts_with(|c: char| c.is_ascii_digit()))
        .then_some(parts[1])
    }) {
        features.push(version.to_owned());
    }
    features
}

fn directory(path: &str) -> Vec<&str> {
    path.rsplit(['/', '\\'])
        .skip(1)
        .filter(|part| !part.is_empty())
        .collect()
}

fn variant(shell: &terminal::Shell, peers: &[&terminal::Shell]) -> Option<String> {
    features(&shell.path).into_iter().find(|feature| {
        peers
            .iter()
            .all(|peer| !features(&peer.path).contains(feature))
    })
}

fn installation(shell: &terminal::Shell, peers: &[&terminal::Shell]) -> Option<String> {
    directory(&shell.path)
        .into_iter()
        .enumerate()
        .find_map(|(depth, part)| {
            peers
                .iter()
                .all(|peer| {
                    directory(&peer.path)
                        .get(depth)
                        .is_none_or(|other| !part.eq_ignore_ascii_case(other))
                })
                .then(|| part.to_owned())
        })
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
