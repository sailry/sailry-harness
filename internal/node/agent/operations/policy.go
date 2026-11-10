package operations

import (
	"encoding/json"
	"sync"

	"github.com/sailry/sailry-harness/internal/protocol"
)

var operationPolicies = sync.OnceValues(func() (map[protocol.ToolOperation]Spec, error) {
	var specs []Spec
	if err := json.Unmarshal(specification, &specs); err != nil {
		return nil, err
	}
	policies := make(map[protocol.ToolOperation]Spec, len(specs))
	for _, spec := range specs {
		policies[spec.Operation] = spec
	}
	return policies, nil
})

func Policy(operation protocol.ToolOperation) (readOnly, concurrent bool) {
	policies, err := operationPolicies()
	if err != nil {
		return false, false
	}
	spec, exists := policies[operation]
	return exists && spec.ReadOnly, exists && spec.Concurrent
}
