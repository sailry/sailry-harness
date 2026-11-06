use crate::{
    Command, NodeId, Output, Request, RequestId, host::installation::Outcome, network::Selection,
    ssh::InstallProgress, wsl::Distribution,
};

#[test]
fn preserves_v1_admission_contracts() {
    let request = RequestId::new();
    for (command, durable) in [
        (Command::ListWslDistributions, false),
        (
            Command::InstallWslHost {
                distribution: "Ubuntu 资料".into(),
                network: None,
            },
            true,
        ),
        (
            Command::InstallWslHost {
                distribution: "Debian".into(),
                network: Some(Selection::Custom {
                    relays: vec!["https://relay.example".into()],
                }),
            },
            true,
        ),
        (Command::ReadWslHostInstall { request }, false),
        (Command::CancelWslHostInstall { request }, true),
    ] {
        assert_eq!(command.durable(), durable);
        let request = Request::new(NodeId([1; 32]), command);
        assert_eq!(request.version, 1);
        let encoded = serde_json::to_vec(&request).unwrap();
        assert_eq!(
            serde_json::from_slice::<Request>(&encoded).unwrap(),
            request
        );
        let value = serde_json::to_value(&request.command).unwrap();
        assert!(value["data"].get("profile").is_none());
        assert!(value["data"].get("credential").is_none());
    }
}

#[test]
fn round_trips_observations_and_outcomes() {
    let node = NodeId([2; 32]);
    for output in [
        Output::WslDistributions(vec![
            Distribution {
                name: "Ubuntu 资料".into(),
                version: 2,
            },
            Distribution {
                name: "Legacy".into(),
                version: 1,
            },
        ]),
        Output::HostInstallProgress(InstallProgress::Installing),
        Output::HostInstallation(Outcome::Installed { node }),
        Output::HostInstallation(Outcome::NetworkUnconfirmed {
            node,
            selection: Selection::Iroh,
        }),
        Output::WslHostInstallCancelled {
            request: RequestId::new(),
        },
    ] {
        assert_eq!(
            serde_json::from_value::<Output>(serde_json::to_value(&output).unwrap()).unwrap(),
            output
        );
    }
}
