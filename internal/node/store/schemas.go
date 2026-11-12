package store

import _ "embed"

//go:embed schema.sql
var NodeSchema string

//go:embed knowledge/schema.sql
var KnowledgeSchema string

//go:embed code_index/schema.sql
var CodeSchema string
