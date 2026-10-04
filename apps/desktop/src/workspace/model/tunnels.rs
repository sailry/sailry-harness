use super::Owner;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tunnel {
    pub owner: Owner,
    pub local_port: u16,
    pub remote_port: u16,
}

pub(crate) struct State {
    pub ports: Vec<u16>,
    pub editable: bool,
    pub loaded: bool,
    pub error: Option<&'static str>,
    pub entries: BTreeMap<usize, Tunnel>,
}

impl State {
    pub fn save_ports(&mut self, original: &[u16], value: &str) -> Result<(), &'static str> {
        if !self.editable || !self.loaded || self.error.is_some() || self.ports != original {
            return Err("tunnel_policy_changed");
        }
        let mut ports = value
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|part| !part.is_empty())
            .map(|part| part.parse::<u16>().ok().filter(|port| *port > 0))
            .collect::<Option<Vec<_>>>()
            .ok_or("tunnel_invalid_ports")?;
        ports.sort_unstable();
        ports.dedup();
        self.ports = ports;
        Ok(())
    }

    pub fn close(&mut self, id: usize, expected: &Tunnel) {
        if self.loaded && self.error.is_none() && self.entries.get(&id) == Some(expected) {
            self.entries.remove(&id);
        }
    }
}

pub(super) fn fixtures() -> BTreeMap<usize, State> {
    (0..2)
        .map(|host| {
            (
                host,
                State {
                    ports: vec![3000, 5173],
                    editable: host == 0,
                    loaded: true,
                    error: None,
                    entries: [3000, 5173]
                        .into_iter()
                        .enumerate()
                        .map(|(id, port)| {
                            (
                                id,
                                Tunnel {
                                    owner: Owner {
                                        host,
                                        project: host,
                                        worktree: host + id * 2,
                                    },
                                    local_port: port + host as u16 * 10000,
                                    remote_port: port,
                                },
                            )
                        })
                        .collect(),
                },
            )
        })
        .collect()
}

pub(crate) fn ports_label(ports: &[u16]) -> String {
    ports
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_validation_is_atomic() {
        let mut data = fixtures();
        let local = data.get_mut(&0).unwrap();
        let original = local.ports.clone();
        for value in ["0", "65536", "-1", "3000,no", "1.5", "3000;5173"] {
            assert_eq!(
                local.save_ports(&original, value),
                Err("tunnel_invalid_ports")
            );
            assert_eq!(local.ports, original);
        }
        local
            .save_ports(&original, "65535, 1 3000\n3000\t5173")
            .unwrap();
        assert_eq!(local.ports, vec![1, 3000, 5173, 65535]);
        assert_eq!(
            local.save_ports(&original, "80"),
            Err("tunnel_policy_changed")
        );
        local.save_ports(&local.ports.clone(), " \n,").unwrap();
        assert!(local.ports.is_empty());
        let remote = data.get_mut(&1).unwrap();
        assert_eq!(
            remote.save_ports(&original, "80"),
            Err("tunnel_policy_changed")
        );
        assert_eq!(remote.ports, original);
    }

    #[test]
    fn preserves_replacements() {
        let mut data = fixtures();
        let local = data.get_mut(&0).unwrap();
        let original = local.entries[&0].clone();
        local.entries.get_mut(&0).unwrap().local_port = 3001;
        local.close(0, &original);
        assert_eq!(local.entries.len(), 2);
        let updated = local.entries[&0].clone();
        local.error = Some("tunnel_load_error");
        local.close(0, &updated);
        assert_eq!(local.entries.len(), 2);
        local.error = None;
        local.close(0, &updated);
        local.close(0, &updated);
        assert_eq!(local.entries.len(), 1);
        assert_eq!(data[&1].entries.len(), 2);
    }
}
