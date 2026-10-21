use super::*;
use serde_json::json;

#[test]
fn distinguishes_authorization_fields_from_public_metadata() {
    let declaration: Declaration = serde_json::from_value(json!({
        "id":"service","display":{"label":"Service"},"accounts":"single",
        "origins":["https://service.example.com"],
        "auth":[{"kind":"application","fields":{"client_id":"app_id","client_secret":"app_secret"},
            "token":{"url":"https://service.example.com/{tenant}/token","body":{"client_id":"{client_id}","client_secret":"{client_secret}"},"access":"/access_token","expires_in":"/expires_in"}}],
        "fields":[{"name":"app_id","title":"App ID"},{"name":"app_secret","title":"App secret","secret":true},{"name":"tenant","title":"Tenant"},{"name":"label","title":"Label"},{"name":"application","title":"Application"}],
        "inject":[{"origin":"https://service.example.com","header":"Authorization","prefix":"Bearer "}]
    })).unwrap();
    let record: Record = serde_json::from_value(json!({
        "id":Id::new(),"plugin":"fixture","connector":"service","revision":4,"auth_revision":2,
        "method":"application","enabled":true,
        "fields":{"app_id":"client","tenant":"current","label":"Original","application":"Original"},
        "secrets":{"app_secret":"private-original"},"grant":null,"grant_revision":1,
        "identity":null,"bound":null,"state":"connected","checked_at_ms":null
    }))
    .unwrap();
    let mut same = record.clone();
    input(&mut same, &declaration, &Input::default()).unwrap();
    assert!(!changed(&record, &same, &declaration));
    input(
        &mut same,
        &declaration,
        &Input {
            fields: BTreeMap::from([
                ("label".into(), "Renamed".into()),
                ("application".into(), "Renamed".into()),
            ]),
            secrets: BTreeMap::from([(
                "app_secret".into(),
                Secret::new("private-original".into()),
            )]),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!changed(&record, &same, &declaration));
    for name in ["app_id", "tenant"] {
        let mut updated = same.clone();
        updated.fields.insert(name.into(), "different".into());
        assert!(changed(&record, &updated, &declaration), "{name}");
    }
    same.secrets.insert(
        "app_secret".into(),
        Secret::new("private-replacement".into()),
    );
    assert!(changed(&record, &same, &declaration));
}
