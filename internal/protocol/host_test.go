package protocol

import (
	"bytes"
	"encoding/json"
	"reflect"
	"testing"
)

func TestHostToolAndSnapshotRoundTrip(t *testing.T) {
	t.Parallel()

	t.Run("host info without tools omits tools and older payload decodes", func(t *testing.T) {
		t.Parallel()
		info := DefaultHostInfo()
		info.Node = NodeID{1}
		info.SampledAtMs = 12345
		encoded, err := json.Marshal(info)
		if err != nil {
			t.Fatal(err)
		}
		if bytes.Contains(encoded, []byte(`"tools"`)) {
			t.Fatalf("empty tools should be omitted: %s", encoded)
		}
		var decoded HostInfo
		if err := json.Unmarshal(encoded, &decoded); err != nil {
			t.Fatal(err)
		}
		if decoded.Tools != nil || decoded.Node != info.Node || decoded.SampledAtMs != info.SampledAtMs {
			t.Fatalf("decoded = %+v", decoded)
		}
	})

	t.Run("host info tools round trip through host_info output and peer tolerance", func(t *testing.T) {
		t.Parallel()
		gitPath := "/usr/bin/git"
		gitVersion := "git version 2.47.0"
		info := DefaultHostInfo()
		info.Node = NodeID{2}
		info.SampledAtMs = 67890
		info.Tools = []HostTool{
			{Name: "fd", Available: false},
			{Name: "git", Available: true, Path: &gitPath, Version: &gitVersion},
		}
		encoded, err := json.Marshal(Output{Kind: "host_info", Data: info})
		if err != nil {
			t.Fatal(err)
		}
		if bytes.Contains(encoded, []byte(`"name":"fd","available":false,"path"`)) || bytes.Contains(encoded, []byte(`"name":"fd","available":false,"version"`)) {
			t.Fatalf("absent path/version should be omitted: %s", encoded)
		}
		var out Output
		if err := json.Unmarshal(encoded, &out); err != nil {
			t.Fatal(err)
		}
		got, ok := out.Data.(*HostInfo)
		if !ok || !reflect.DeepEqual(*got, info) {
			t.Fatalf("round trip mismatch:\ngot:  %#v\nwant: %#v", out.Data, info)
		}

		// Peer tolerance: future members on HostTool and HostInfo decode cleanly.
		widened := bytes.Replace(encoded, []byte(`"name":"git"`), []byte(`"name":"git","arch":"arm64"`), 1)
		var peerOut Output
		if err := DecodeAs(PeerData, widened, &peerOut); err != nil {
			t.Fatalf("widened host_info rejected: %v", err)
		}
		peerInfo := peerOut.Data.(*HostInfo)
		if !reflect.DeepEqual(peerInfo.Tools, info.Tools) {
			t.Fatalf("widened tools = %#v, want %#v", peerInfo.Tools, info.Tools)
		}

		// Required fields on HostTool are enforced.
		for name, broken := range map[string]string{
			"missing name":      `{"available":true}`,
			"missing available": `{"name":"git"}`,
			"null name":         `{"name":null,"available":true}`,
		} {
			t.Run(name, func(t *testing.T) {
				if err := json.Unmarshal([]byte(broken), new(HostTool)); err == nil {
					t.Fatalf("expected error for %s", broken)
				}
			})
		}
	})

	t.Run("snapshot project_sources and host_tools round trip and preserve older snapshots", func(t *testing.T) {
		t.Parallel()
		older := DefaultSnapshot()
		older.Node = NodeID{3}
		olderBytes, err := json.Marshal(older)
		if err != nil {
			t.Fatal(err)
		}
		if bytes.Contains(olderBytes, []byte(`"project_sources"`)) || bytes.Contains(olderBytes, []byte(`"host_tools"`)) {
			t.Fatalf("empty project_sources/host_tools should be omitted: %s", olderBytes)
		}
		var decodedOlder Snapshot
		if err := DecodeAs(PeerData, olderBytes, &decodedOlder); err != nil {
			t.Fatal(err)
		}
		if decodedOlder.ProjectSources != nil || decodedOlder.HostTools != nil {
			t.Fatalf("older snapshot unexpectedly populated fields: %+v %+v", decodedOlder.ProjectSources, decodedOlder.HostTools)
		}

		pyPath := "/opt/bin/python3"
		pyVersion := "Python 3.12.7"
		snap := DefaultSnapshot()
		snap.Node = NodeID{4}
		snap.ProjectSources = []string{"local", "clone"}
		snap.HostTools = []HostTool{
			{Name: "git", Available: false},
			{Name: "python", Available: true, Path: &pyPath, Version: &pyVersion},
		}
		encoded, err := json.Marshal(Output{Kind: "snapshot", Data: snap})
		if err != nil {
			t.Fatal(err)
		}
		var out Output
		if err := json.Unmarshal(encoded, &out); err != nil {
			t.Fatal(err)
		}
		got := out.Data.(*Snapshot)
		if !reflect.DeepEqual(got.ProjectSources, snap.ProjectSources) || !reflect.DeepEqual(got.HostTools, snap.HostTools) {
			t.Fatalf("snapshot round trip mismatch:\ngot:  sources=%v tools=%#v\nwant: sources=%v tools=%#v", got.ProjectSources, got.HostTools, snap.ProjectSources, snap.HostTools)
		}

		widened := bytes.Replace(encoded, []byte(`"name":"python"`), []byte(`"name":"python","extra_meta":{"managed":true}`), 1)
		var peerOut Output
		if err := DecodeAs(PeerData, widened, &peerOut); err != nil {
			t.Fatalf("widened snapshot rejected: %v", err)
		}
		peerSnap := peerOut.Data.(*Snapshot)
		if !reflect.DeepEqual(peerSnap.ProjectSources, snap.ProjectSources) || !reflect.DeepEqual(peerSnap.HostTools, snap.HostTools) {
			t.Fatalf("widened snapshot lost fields: sources=%v tools=%#v", peerSnap.ProjectSources, peerSnap.HostTools)
		}

		nullSource := bytes.Replace(encoded, []byte(`"clone"`), []byte(`null`), 1)
		if err := json.Unmarshal(nullSource, new(Output)); err == nil {
			t.Fatal("snapshot with null element in project_sources was accepted")
		}
	})
}
