use std::ffi::OsString;
use std::path::PathBuf;

pub(crate) struct Options {
    pub(crate) data_dir: PathBuf,
    pub(crate) network: sailry_node_runtime::NetworkScope,
    pub(crate) internet: bool,
    pub(crate) bootstrap: bool,
    pub(crate) pairing: Option<sailry_link::rendezvous::Relay>,
}

pub(crate) fn parse(args: impl Iterator<Item = OsString>) -> Result<Option<Options>, String> {
    let mut args = args.peekable();
    if args
        .peek()
        .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        args.next();
        if args.next().is_some() {
            return Err("--help cannot be combined with other options".into());
        }
        println!(
            "Usage: sailry-host [--data-dir <absolute private directory>] [--bind <IP:port> | --internet | --relay <HTTPS URL> ...] [--pairing-service <HTTPS origin> | --bootstrap]\nDefaults to ~/.sailry and loopback. No interactive Agent.\nPairing is opt-in: prints a private PIN, refreshes every 60 seconds until paired or stopped."
        );
        return Ok(None);
    }
    let mut data_dir = None;
    let mut bind = None;
    let mut internet = false;
    let mut relays = Vec::new();
    let mut pairing = None;
    let mut bootstrap = false;
    while let Some(argument) = args.next() {
        if argument == "--bootstrap" && !bootstrap {
            bootstrap = true;
            continue;
        }
        if argument == "--pairing-service" {
            if pairing.is_some() {
                return Err("--pairing-service must be specified only once".into());
            }
            let origin = args.next().ok_or("--pairing-service requires an origin")?;
            pairing = Some(
                sailry_link::rendezvous::Relay::new(
                    origin.to_str().ok_or("pairing origin must be UTF-8")?,
                )
                .map_err(|_| "invalid pairing service origin")?,
            );
            continue;
        }
        if argument == "--internet" && !internet {
            internet = true;
            continue;
        }
        if argument == "--relay" {
            relays.push(
                args.next()
                    .ok_or("--relay requires a URL")?
                    .into_string()
                    .map_err(|_| "--relay must be UTF-8")?,
            );
            continue;
        }
        if argument == "--bind" && bind.is_none() {
            let value = args.next().ok_or("--bind requires an IP:port address")?;
            bind = Some(
                value
                    .to_str()
                    .ok_or("--bind must be UTF-8")?
                    .parse()
                    .map_err(|_| "--bind requires a valid IP:port address")?,
            );
            continue;
        }
        if argument != "--data-dir" || data_dir.is_some() {
            return Err("unknown or repeated option; use --help for usage".into());
        }
        let value = args.next().ok_or("--data-dir requires a path")?;
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err("--data-dir must be an absolute path".into());
        }
        data_dir = Some(path);
    }
    if (bind.is_some() && (internet || !relays.is_empty())) || (internet && !relays.is_empty()) {
        return Err("choose only one of --bind, --internet or --relay".into());
    }
    if bootstrap && pairing.is_some() {
        return Err("choose bootstrap or pairing service".into());
    }
    let use_relay = internet || !relays.is_empty();
    let network = if !relays.is_empty() {
        sailry_node_runtime::NetworkScope::CustomRelays(relays)
    } else if internet {
        sailry_node_runtime::NetworkScope::Internet
    } else {
        sailry_node_runtime::NetworkScope::Direct(
            bind.unwrap_or_else(|| ([127, 0, 0, 1], 0).into()),
        )
    };
    Ok(Some(Options {
        data_dir: match data_dir {
            Some(path) => path,
            None => sailry_node_runtime::default_data_dir().map_err(|error| error.to_string())?,
        },
        network,
        internet: use_relay,
        pairing,
        bootstrap,
    }))
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn uses_shared_default() {
        let options = parse(std::iter::empty()).unwrap().unwrap();
        assert_eq!(
            options.data_dir,
            sailry_node_runtime::default_data_dir().unwrap()
        );
        assert!(!options.internet);
        assert!(options.pairing.is_none());
    }

    #[test]
    fn help_does_not_start() {
        for flag in ["--help", "-h"] {
            assert!(parse([flag.into()].into_iter()).unwrap().is_none());
            assert!(parse([flag.into(), "--data-dir".into()].into_iter()).is_err());
        }
    }

    #[test]
    fn rejects_invalid_options() {
        for args in [
            vec!["--data-dir"],
            vec!["--data-dir", "relative"],
            vec!["--data-dir", "/first", "--data-dir", "/second"],
            vec!["--prompt", "unexpected"],
        ] {
            assert!(parse(args.into_iter().map(Into::into)).is_err());
        }
    }

    #[test]
    fn accepts_explicit_profile() {
        let path = std::env::temp_dir().join("profile with spaces");
        let options = parse(["--data-dir".into(), path.clone().into_os_string()].into_iter())
            .unwrap()
            .unwrap();
        assert_eq!(options.data_dir, path);
        assert!(options.pairing.is_none());
    }

    #[test]
    fn checks_pairing_origin() {
        for origin in ["https://pair.example", "http://127.0.0.1:8080"] {
            assert!(
                parse(
                    ["--data-dir", "/profile", "--pairing-service", origin]
                        .into_iter()
                        .map(Into::into)
                )
                .unwrap()
                .unwrap()
                .pairing
                .is_some()
            );
        }
        for values in [
            vec!["--pairing-service"],
            vec!["--pairing-service", "http://pair.example"],
            vec!["--pairing-service", "https://pair.example/path"],
            vec![
                "--pairing-service",
                "https://pair.example",
                "--pairing-service",
                "https://pair.example",
            ],
        ] {
            assert!(
                parse(
                    ["--data-dir", "/profile"]
                        .into_iter()
                        .chain(values)
                        .map(Into::into)
                )
                .is_err()
            );
        }
    }

    #[test]
    fn bootstrap_is_explicit_and_exclusive() {
        assert!(
            parse(["--bootstrap".into()].into_iter())
                .unwrap()
                .unwrap()
                .bootstrap
        );
        assert!(parse(["--bootstrap", "--bootstrap"].into_iter().map(Into::into)).is_err());
        assert!(
            parse(
                [
                    "--bootstrap",
                    "--pairing-service",
                    "https://link.sailry.dev"
                ]
                .into_iter()
                .map(Into::into)
            )
            .is_err()
        );
    }

    #[test]
    fn checks_network_mode() {
        let options = parse(
            [
                "--data-dir",
                "/profile",
                "--relay",
                "https://one.example",
                "--relay",
                "https://two.example",
            ]
            .into_iter()
            .map(Into::into),
        )
        .unwrap()
        .unwrap();
        assert!(options.internet);
        assert!(
            matches!(options.network, sailry_node_runtime::NetworkScope::CustomRelays(urls) if urls.len() == 2)
        );
        for args in [
            vec![
                "--data-dir",
                "/profile",
                "--internet",
                "--relay",
                "https://one.example",
            ],
            vec![
                "--data-dir",
                "/profile",
                "--bind",
                "127.0.0.1:0",
                "--internet",
            ],
        ] {
            assert!(parse(args.into_iter().map(Into::into)).is_err());
        }
    }
}
