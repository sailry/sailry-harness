package testplugin

const ModelName = "model-fixture"

func Model() Package {
	return Package{Name: ModelName, Platform: `{
  "api_version":"v1","scope":"desktop","actions":["models.generate"],
  "display":{"label":"Model fixture","icon":"phosphor:cpu"},
  "settings_schema":"dev.sailry.platform/settings.json",
  "desktop":{"entry":"dev.sailry.platform/model.js","resources":["dev.sailry.platform/model.js"],
    "navigation":{"label":"Model fixture","icon":"reicon:devices/cpu"},"navigation_options":{"pinned":true,"order":30}}
}`, Files: map[string]string{
		"dev.sailry.platform/settings.json": `{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,"properties":{"model":{"type":"string","title":"Model","x-sailry-model":true,"minLength":1},"model_effort":{"type":"string","title":"Effort","x-sailry-model-effort":"model","default":"none"}}}`,
		"dev.sailry.platform/model.js": `
import {View,div} from 'gpui-kit';
import {Button} from 'gpui-component';
import {readSettings,listModels,resolveModel,prepareRequest,completeRequest,forgetRequest} from 'sailry/sdk';
export default class ModelFixture extends View {
  init(_props,cx){this.ready=false;this.busy=false;this.output='';cx.spawn(async cx=>{
    const [settings,catalog]=await Promise.all([readSettings(),listModels()]);
    this.model=resolveModel(settings,catalog,'model');this.ready=true;cx.notify();});}
  generate(cx){if(!this.ready||this.busy)return;this.busy=true;cx.notify();cx.spawn(async cx=>{
    const id=prepareRequest({kind:'generate_plugin_text',data:{model:this.model.model,effort:this.model.effort,prompt:'Return the fixture reply.'}});
    try {const result=await completeRequest(id);if(result.Err)throw new Error(result.Err.message);this.output=result.Ok.data.text;}
    finally {forgetRequest(id);this.busy=false;cx.notify();}});}
  render(){return div().v_flex().gap(12).p(24)
    .child(new Button('generate-fixture').label('Generate fixture').disabled(!this.ready||this.busy).on_click((_event,cx)=>this.generate(cx)))
    .child(this.output);}
}`,
	}}
}
