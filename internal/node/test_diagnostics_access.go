package node

func (access TestAccess) LoggingEnabled() bool { return access.node.logs }
