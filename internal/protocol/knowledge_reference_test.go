package protocol

import (
	"encoding/json"
	"testing"
)

func TestKnowledgeReferencesAndLegacyPrivateDefault(t *testing.T) {
	t.Parallel()
	var definition KnowledgeDefinition
	if err := json.Unmarshal([]byte(`{"name":"Legacy","source":{"kind":"directory","path":"/notes"}}`), &definition); err != nil {
		t.Fatal(err)
	}
	if definition.Visibility != KnowledgeVisibilityPrivate {
		t.Fatalf("legacy visibility = %q", definition.Visibility)
	}
	node, id := NodeID{4}, NewKnowledgeID()
	query := DefaultKnowledgeQuery()
	query.Query, query.Refs = "notes", []KnowledgeRef{{Node: node, ID: id}}
	data, err := json.Marshal(query)
	if err != nil {
		t.Fatal(err)
	}
	var decoded KnowledgeQuery
	if err := json.Unmarshal(data, &decoded); err != nil {
		t.Fatal(err)
	}
	if len(decoded.Refs) != 1 || decoded.Refs[0] != query.Refs[0] {
		t.Fatalf("reference lost storage identity: %+v", decoded.Refs)
	}
	path := HostPath{Node: node, Path: "/notes"}
	data, err = json.Marshal(path)
	if err != nil {
		t.Fatal(err)
	}
	var decodedPath HostPath
	if err := json.Unmarshal(data, &decodedPath); err != nil || decodedPath != path {
		t.Fatalf("host path: %+v: %v", decodedPath, err)
	}
}
