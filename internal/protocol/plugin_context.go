package protocol

func (scope PluginContext) ControllerSettings() bool {
	return scope.Surface.Kind == "settings" && scope.Invocation == nil && scope.Turn == nil &&
		scope.Session == nil && scope.Worktree == nil
}
