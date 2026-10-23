use super::server_form_bound;
use sailry_protocol::plugin::Info;
use serde_json::json;

#[test]
fn routes_only_bound_servers_to_protected_forms() {
    let mut info: Info = serde_json::from_value(json!({
        "summary":{
            "name":"fixture","revision":1,"digest":"digest","settings_revision":0,
            "enabled":true,"version":"0.1.0","description":null
        },
        "icon":null,"extension":null,"settings":{
            "$schema":"https://json-schema.org/draft/2020-12/schema",
            "type":"object","additionalProperties":false,"properties":{
                "token":{
                    "type":"string","x-sailry-secret":{
                        "server":"private","header":"Authorization"
                    }
                }
            }
        }
    }))
    .unwrap();
    assert!(server_form_bound(&info, "private"));
    assert!(!server_form_bound(&info, "public"));
    info.settings = None;
    assert!(!server_form_bound(&info, "private"));
}
