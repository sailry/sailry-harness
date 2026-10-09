use super::*;

#[tokio::test]
async fn shares_runtime_configuration_and_conflicts() {
    let fixture = Fixture::start().await;
    let [local, remote] = fixture.clients();
    for (writer, reader) in [(&local, &remote), (&remote, &local)] {
        let Output::PythonSettings(previous) = execute(writer, Command::ReadPythonSettings).await
        else {
            panic!("Python settings expected")
        };
        let mut changed = previous.clone();
        changed.index = Some("https://pypi.org/simple/".into());
        let request = writer.prepare(Command::SavePythonSettings(changed));
        let admitted = writer.dispatch(request.clone()).await.unwrap();
        assert!(admitted.receipt.durable);
        let output = admitted.completion.await.unwrap().unwrap();
        assert_eq!(writer.execute(request.clone()).await.unwrap(), output);
        assert_eq!(execute(reader, Command::ReadPythonSettings).await, output);
        let Output::PythonSettings(mut newer) = output.clone() else {
            panic!("Python settings expected")
        };
        newer.index = Some("https://pypi.tuna.tsinghua.edu.cn/simple/".into());
        let current = execute(reader, Command::SavePythonSettings(newer)).await;
        assert_eq!(writer.execute(request).await.unwrap(), output);
        assert_eq!(execute(writer, Command::ReadPythonSettings).await, current);
        assert_eq!(execute(reader, Command::ReadPythonSettings).await, current);
        assert_eq!(
            writer
                .execute(writer.prepare(Command::SavePythonSettings(previous)))
                .await
                .unwrap_err()
                .code,
            ErrorCode::RevisionConflict
        );
    }
    fixture.close().await;
}
