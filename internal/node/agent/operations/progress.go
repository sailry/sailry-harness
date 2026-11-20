package operations

import "github.com/sailry/sailry-harness/internal/protocol"

const UpdatePlanTool = "update_plan"

// PresentFilesTool is the Node's file delivery; its results show as file cards.
const PresentFilesTool = "present_files"

var LegacyUpdatePlanTool = Alias("progress", UpdatePlanTool)

func IsUpdatePlanTool(name string) bool {
	return name == UpdatePlanTool || name == LegacyUpdatePlanTool
}

// UpdatePlanSchema uses the same argument contract as progress.update.
func UpdatePlanSchema() string {
	specs, err := operationPolicies()
	if err != nil {
		panic(err)
	}
	return string(specs[protocol.ToolOperationProgress].Parameters)
}
