package protocol

import (
	"encoding/json"
	"testing"
)

func TestDispatchExecutionNodeRemainsOptionalAndExact(t *testing.T) {
	node := NodeID{2, 7}
	cases := []struct {
		name     string
		body     string
		location func([]byte) (*NodeID, error)
	}{
		{"handler", `{"name":"queue","revision":1,"enabled":true,"source":{"package":"fixture","topic":"run"},"queue":"queue","callback":{"scope":{},"completion":"command","command":{"kind":"snapshot"},"bindings":{}}}`,
			func(body []byte) (*NodeID, error) {
				var value DispatchHandler
				err := json.Unmarshal(body, &value)
				return value.ExecutionNode, err
			}},
		{"schedule", `{"id":"ba13dc3c-92fe-430c-92b8-8b0bd5edcaaf","revision":1,"enabled":true,"topic":"run","payload":null,"timing":{"kind":"once","data":{"at_ms":1000}}}`,
			func(body []byte) (*NodeID, error) {
				var value DispatchSchedule
				err := json.Unmarshal(body, &value)
				return value.ExecutionNode, err
			}},
		{"job", `{"id":"ba13dc3c-92fe-430c-92b8-8b0bd5edcaaf","event":{"id":"3d9b377f-d4a2-4b07-a5b9-8aff27ec8c2c","source":{"package":"fixture","topic":"run"},"payload":null,"timestamp_ms":0},"handler":"run","queue":"queue","request":"fe6d05fb-939b-4e67-8889-fbc8bd1845b5","status":"queued","created_ms":0}`,
			func(body []byte) (*NodeID, error) {
				var value DispatchJob
				err := json.Unmarshal(body, &value)
				return value.ExecutionNode, err
			}},
	}
	for _, test := range cases {
		t.Run(test.name, func(t *testing.T) {
			location, err := test.location([]byte(test.body))
			if err != nil || location != nil {
				t.Fatalf("legacy dispatch location changed: %v, %v", location, err)
			}
			var object map[string]json.RawMessage
			if err := json.Unmarshal([]byte(test.body), &object); err != nil {
				t.Fatal(err)
			}
			object["execution_node"], err = json.Marshal(node)
			if err != nil {
				t.Fatal(err)
			}
			body, err := json.Marshal(object)
			if err != nil {
				t.Fatal(err)
			}
			location, err = test.location(body)
			if err != nil || location == nil || *location != node {
				t.Fatalf("dispatch location did not preserve the exact Node: %v, %v", location, err)
			}
			object["execution_node"] = json.RawMessage("[2,7]")
			body, err = json.Marshal(object)
			if err != nil {
				t.Fatal(err)
			}
			if _, err := test.location(body); err == nil {
				t.Fatal("dispatch location accepted an incomplete Node identity")
			}
		})
	}
}
