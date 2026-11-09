// Reproduce the full controller preference fixture with the pinned serde format.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Serialize, Deserialize)]
struct Data {
    version:u32, notifications:[bool;4], toast_seconds:u64, pairing:String,
    iroh_relays:Option<Vec<String>>, devices:Option<BTreeMap<String,Device>>,
    terminal:Terminal, appearance:Selection, theme_effects:Option<bool>,
    composer:Option<Composer>, feature_pins:Option<BTreeMap<String,bool>>,
    feature_order:Option<Vec<String>>, browser_persistent:Option<bool>,
    surfaces:Option<Surfaces>, shortcuts:Option<BTreeMap<String,String>>,
    dictation:Option<Dictation>, #[serde(alias="sessions")] pins:Option<BTreeMap<String,Pin>>,
    artifact_drafts:Option<BTreeMap<String,Draft>>, plugin_directories:Option<BTreeMap<String,Directory>>,
    message_display:Option<MessageDisplay>, session_order:Option<SessionOrder>,
    message_text_size:Option<u8>, file_picker:Option<FilePicker>, language:Option<Language>,
    update_channel:Option<Channel>, welcome_seen:Option<bool>, #[serde(default)] sidebar_metrics:bool,
}
#[derive(Serialize,Deserialize)] struct Device {name:Option<String>,execution:bool,platform:Option<String>}
#[derive(Serialize,Deserialize)] struct Terminal {font_size:u8,font_family:String,paste_protection:bool}
#[derive(Serialize,Deserialize)] struct Selection {package:String,mode:Mode}
#[derive(Serialize,Deserialize)] #[serde(rename_all="snake_case")] enum Mode {System,Light,Dark}
#[derive(Serialize,Deserialize)] struct Surfaces {main:f32,sidebar:Option<f32>}
#[derive(Serialize,Deserialize)] struct Composer {model:Option<Model>,mode:WorkMode,permission:Permission}
#[derive(Serialize,Deserialize)] struct Model {source:[u8;32],provider:String,model:String,effort:Effort,#[serde(default)] features:BTreeMap<String,String>}
#[derive(Serialize,Deserialize)] #[serde(rename_all="snake_case")] enum WorkMode {Plan,Code,Flagship}
#[derive(Serialize,Deserialize)] #[serde(rename_all="snake_case")] enum Permission {Ask,Project,Full}
#[derive(Serialize,Deserialize)] #[serde(rename_all="snake_case")] enum Effort {Default,None,Minimal,Low,Medium,High,Xhigh,Max,Budget(i32)}
#[derive(Serialize,Deserialize)] struct Dictation {microphone:Option<String>,language:SpeechLanguage,#[serde(default)] model:Option<String>}
#[derive(Serialize,Deserialize)] #[serde(rename_all="snake_case")] enum SpeechLanguage {Auto,Chinese,English}
#[derive(Serialize,Deserialize)] struct Pin {pinned:bool,#[serde(default)] order:Option<u64>}
#[derive(Serialize,Deserialize)] #[serde(deny_unknown_fields)] struct Field {name:String,values:Vec<String>}
#[derive(Serialize,Deserialize)] #[serde(deny_unknown_fields)] struct Draft {form:String,fields:Vec<Field>}
#[derive(Serialize,Deserialize)] struct Directory {digest:String,install_revision:u64,path:PathBuf}
#[derive(Serialize,Deserialize)] #[serde(rename_all="snake_case")] enum MessageDisplay {Detailed,Compact}
#[derive(Serialize,Deserialize)] #[serde(rename_all="snake_case")] enum SessionOrder {Created,Updated}
#[derive(Serialize,Deserialize)] struct FilePicker {grid:bool,hidden:bool}
#[derive(Serialize,Deserialize)] #[serde(rename_all="snake_case")] enum Language {System,English,Chinese,TraditionalChinese,Japanese,Korean,French,German,Spanish,PortugueseBrazil,Russian}
#[derive(Serialize,Deserialize)] #[serde(rename_all="snake_case")] enum Channel {Stable,Experience}
fn main(){
    let data:Data=serde_json::from_str(include_str!("preferences-rust.json")).unwrap();
    println!("{}",serde_json::to_string_pretty(&data).unwrap());
}
