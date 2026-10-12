use super::*;
use chrono::{Datelike, Timelike};
use chrono_tz::Asia::Shanghai;

fn timing() -> Value {
    json!({"kind":"weekly","data":{"timezone":"Asia/Shanghai","weekdays":[1,2,3,4,5],"hour":9,"minute":0}})
}

#[tokio::test]
async fn edits_preserve_existing_tasks_and_cursors() {
    for remote in [false, true] {
        let (directory, node, controller, client, tree) = fixture(remote).await;
        let context = installed(&client, directory.path(), tree).await;
        let configuration = config(&client, "http://127.0.0.1:1/v1".into()).await;
        let once = ok(
            &client,
            &context,
            "save",
            draft(Some(configuration.clone())),
        )
        .await;
        let mut fixed = draft(Some(configuration.clone()));
        fixed["timing"] = json!({"kind":"every","data":{"anchor_ms":1,"interval_ms":86_400_000}});
        let fixed = ok(&client, &context, "save", fixed).await;
        let originals = ok(&client, &context, "list", json!({})).await;
        let mut input = draft(Some(configuration.clone()));
        input["timing"] = timing();
        let before = chrono::Utc::now().timestamp_millis();
        let mut item = ok(&client, &context, "save", input).await;
        let listed = ok(&client, &context, "list", json!({})).await;
        let current = listed["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|task| task["id"] == item["id"])
            .unwrap();
        let cursor = current["next_ms"].as_i64().unwrap();
        assert!(cursor > before);
        let local = chrono::DateTime::from_timestamp_millis(cursor)
            .unwrap()
            .with_timezone(&Shanghai);
        assert!(local.weekday().number_from_monday() <= 5);
        assert_eq!((local.hour(), local.minute()), (9, 0));
        item["name"] = json!("Renamed weekly task");
        item = ok(&client, &context, "save", item).await;
        assert_eq!(item["timing"], timing());
        assert_eq!(item["config"], json!(configuration));
        let retained = ok(&client, &context, "list", json!({})).await;
        assert_eq!(
            retained["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|task| task["id"] == item["id"])
                .unwrap()["next_ms"],
            cursor
        );
        drop(client);
        node.shutdown().await.unwrap();
        let node = Node::start(directory.path().join("node")).await.unwrap();
        let client = Client::new(if remote {
            controller.handle().remote(node.link().address())
        } else {
            node.local()
        });
        assert_eq!(ok(&client, &context, "list", json!({})).await, retained);
        ok(
            &client,
            &context,
            "remove",
            json!({"id":item["id"],"revision":item["revision"]}),
        )
        .await;
        assert_eq!(ok(&client, &context, "list", json!({})).await, originals);
        let jobs::Output::Schedules(schedules) =
            dispatch::run(&client, &context, jobs::Command::ListSchedules).await
        else {
            panic!("schedules expected")
        };
        assert_eq!(schedules.len(), 2);
        assert!(
            schedules
                .iter()
                .any(|schedule| json!(schedule.id) == once["id"]
                    && matches!(&schedule.timing, jobs::Timing::Once { .. }))
        );
        assert!(
            schedules
                .iter()
                .any(|schedule| json!(schedule.id) == fixed["id"]
                    && matches!(&schedule.timing, jobs::Timing::Every { .. }))
        );
        assert!(history(&client, &context).await.is_empty());
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}

#[tokio::test]
async fn invalid_timing_rolls_back_all_resources() {
    for remote in [false, true] {
        let (directory, node, controller, client, tree) = fixture(remote).await;
        let context = installed(&client, directory.path(), tree).await;
        let configuration = config(&client, "http://127.0.0.1:1/v1".into()).await;
        ok(
            &client,
            &context,
            "save",
            draft(Some(configuration.clone())),
        )
        .await;
        let original = ok(&client, &context, "list", json!({})).await;
        let handlers = dispatch::run(&client, &context, jobs::Command::ListHandlers).await;
        let schedules = dispatch::run(&client, &context, jobs::Command::ListSchedules).await;
        for invalid in [
            json!({"kind":"weekly","data":{"timezone":"Mars/Olympus","weekdays":[1],"hour":9,"minute":0}}),
            json!({"kind":"weekly","data":{"timezone":"UTC","weekdays":[],"hour":9,"minute":0}}),
        ] {
            let mut input = draft(Some(configuration.clone()));
            let id = input["id"].as_str().unwrap().to_owned();
            input["timing"] = invalid;
            assert_eq!(
                call(&client, &context, "save", input).await["Err"]["code"],
                "invalid_request"
            );
            let Output::PluginValue(absent) = client
                .execute(
                    client
                        .prepare(Command::ReadPluginValue {
                            key: format!("task/{id}"),
                        })
                        .with_plugin(context.clone()),
                )
                .await
                .unwrap()
            else {
                panic!("value expected")
            };
            assert!(!absent.present);
            assert_eq!(absent.revision, 0);
            assert_eq!(ok(&client, &context, "list", json!({})).await, original);
            assert_eq!(
                dispatch::run(&client, &context, jobs::Command::ListHandlers).await,
                handlers
            );
            assert_eq!(
                dispatch::run(&client, &context, jobs::Command::ListSchedules).await,
                schedules
            );
        }
        node.shutdown().await.unwrap();
        controller.close().await.unwrap();
    }
}
