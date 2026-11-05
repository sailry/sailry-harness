use super::*;

#[test]
fn accepts_bound_addresses() {
    let address = EndpointAddr::new(iroh::SecretKey::generate().public())
        .with_ip_addr("127.0.0.1:1234".parse().unwrap());
    let id = NodeId(*address.id.as_bytes());
    let bytes = serde_json::to_vec(&Record {
        version: 1,
        peers: vec![Peer {
            id,
            address: Some(address.clone()),
        }],
    })
    .unwrap();
    let peers = Record::decode(&bytes).unwrap();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].id, id);
    assert_eq!(peers[0].address, Some(address));
}

#[test]
fn rejects_duplicate_identities() {
    let peer = Peer {
        id: NodeId([1; 32]),
        address: None,
    };
    let bytes = serde_json::to_vec(&Record {
        version: 1,
        peers: vec![peer.clone(), peer],
    })
    .unwrap();
    assert!(Record::decode(&bytes).is_err());
}

#[test]
fn rejects_mismatched_addresses() {
    let address = EndpointAddr::new(iroh::SecretKey::generate().public());
    let mut id = NodeId(*address.id.as_bytes());
    id.0[0] ^= 1;
    let bytes = serde_json::to_vec(&Record {
        version: 1,
        peers: vec![Peer {
            id,
            address: Some(address),
        }],
    })
    .unwrap();
    assert!(Record::decode(&bytes).is_err());
}

#[test]
fn bounds_peer_count() {
    let mut record = Record {
        version: 1,
        peers: (0..MAX_PEERS)
            .map(|index| Peer {
                id: NodeId([index as u8; 32]),
                address: None,
            })
            .collect(),
    };
    let bytes = serde_json::to_vec(&record).unwrap();
    assert_eq!(Record::decode(&bytes).unwrap().len(), MAX_PEERS);
    record.peers.push(Peer {
        id: NodeId([255; 32]),
        address: None,
    });
    assert!(Record::decode(&serde_json::to_vec(&record).unwrap()).is_err());
}

#[test]
fn bounds_record_bytes() {
    let mut bytes = br#"{"version":1,"peers":[]}"#.to_vec();
    bytes.resize(MAX_BYTES as usize, b' ');
    assert!(Record::decode(&bytes).unwrap().is_empty());
    bytes.push(b' ');
    assert!(Record::decode(&bytes).is_err());
}

#[test]
fn rejects_invalid_format() {
    for bytes in [
        b"truncated".as_slice(),
        br#"{"version":2,"peers":[]}"#,
        br#"{"version":1}"#,
    ] {
        assert!(Record::decode(bytes).is_err());
    }
}
