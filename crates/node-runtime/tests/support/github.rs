//! An isolated CLI exercises login commands without reading a real account.
#[path = "environment.rs"]
mod environment;
use sailry_node_runtime::EnvironmentFixture;
use std::path::Path;

pub const TOKEN: &str = "synthetic-gh-private-token";

pub struct Cli {
    login: environment::Login,
}

impl Cli {
    pub fn new(directory: &Path) -> Self {
        let login = environment::Login::new(directory);
        environment::executable(
            &login.first.join("gh"),
            concat!(
                "#!/bin/sh\n",
                "printf '%s\\n' \"$*\" >> \"$HOME/gh-calls\"\n",
                "printf '%s:%s:%s\\n' \"$HOME\" \"${LOGIN_SECRET-unset}\" \"${SAILRY_MODEL_API_KEY-unset}\" > \"$HOME/gh-environment\"\n",
                "case \"$*\" in\n",
                "'auth status --active --hostname github.com')\n",
                "  printf 'synthetic-gh-private-token\\n'\n",
                "  printf 'synthetic-gh-private-token\\n' >&2\n",
                "  test ! -f \"$HOME/gh-signed-out\";;\n",
                "'auth token --hostname github.com')\n",
                "  if test -f \"$HOME/gh-fail\"; then printf 'synthetic-gh-private-token\\n' >&2; exit 1; fi\n",
                "  if test -f \"$HOME/gh-hold\"; then\n",
                "    : > \"$HOME/gh-entered\"\n",
                "    while test ! -f \"$HOME/gh-release\"; do /bin/sleep 0.02; done\n",
                "  fi\n",
                "  if test -f \"$HOME/gh-oversized\"; then /usr/bin/head -c 5000 /dev/zero | /usr/bin/tr '\\000' x; exit 0; fi\n",
                "  printf 'synthetic-gh-private-token\\n';;\n",
                "*) exit 2;;\n",
                "esac\n",
            ),
        );
        Self { login }
    }

    pub fn source(&self) -> EnvironmentFixture {
        self.login.source()
    }
    pub fn home(&self) -> &Path {
        &self.login.home
    }
    pub fn calls(&self, action: &str) -> usize {
        std::fs::read_to_string(self.home().join("gh-calls"))
            .unwrap_or_default()
            .lines()
            .filter(|line| line.starts_with(&format!("auth {action} ")))
            .count()
    }
}
