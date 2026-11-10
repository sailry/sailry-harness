package plugins

import "github.com/santhosh-tekuri/jsonschema/v6"

func compileSchemaAt(root any, pointer string) (*jsonschema.Schema, error) {
	compiler := jsonschema.NewCompiler()
	compiler.UseLoader(jsonschema.SchemeURLLoader{})
	if err := compiler.AddResource("urn:sailry:plugin:storage", root); err != nil {
		return nil, err
	}
	return compiler.Compile("urn:sailry:plugin:storage" + pointer)
}
