//! The storage actor settles native timeouts without recording a user answer.
use super::*;
use sailry_link::CancellationToken;

impl Ingress {
    pub(crate) async fn expire_question(
        &self,
        turn: TurnId,
        call: String,
        stop: CancellationToken,
    ) -> Result<Question, Fault> {
        let (reply, response) = oneshot::channel();
        self.submit(Job::Agent(Box::new(Operation::ExpireQuestion {
            turn,
            call,
            stop,
            reply,
        })))
        .await
        .map_err(|_| super::super::super::unavailable())?;
        response
            .await
            .map_err(|_| super::super::super::unavailable())?
    }
}

pub(in crate::store::agent) async fn expire(
    database: &mut Database,
    turn: TurnId,
    call: &str,
    stop: &CancellationToken,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Question, Fault> {
    let publication = database.publication.clone();
    let mut write = Some(database.write().await?);
    let result: Result<_, Fault> = async {
        let mut transaction = database
            .connection
            .unchecked_transaction()
            .await
            .map_err(storage_error)?;
        transaction.set_drop_behavior(turso::transaction::DropBehavior::Ignore);
        let mut question: Question = {
            let mut statement = transaction
                .prepare("SELECT body FROM agent_questions WHERE turn=?1 AND call=?2")
                .await
                .map_err(storage_error)?;
            let row = statement
                .query_row(params![turn.to_string(), call])
                .await
                .map_err(storage_error)?;
            let body: Vec<u8> = row.get(0).map_err(storage_error)?;
            serde_json::from_slice(&body).map_err(storage_error)?
        };
        if question.state != State::Pending {
            return Ok(question);
        }
        if stop.is_cancelled() {
            return Err(Fault::new(ErrorCode::Cancelled, "question was cancelled"));
        }
        calls::active(&transaction, turn).await?;
        if matches!(
            spec(&transaction, &question).await?.input,
            question::Input::Form { .. } | question::Input::Url { .. }
        ) {
            return Err(Fault::invalid(
                "MCP input does not use clarification timeouts",
            ));
        }
        question.state = State::TimedOut;
        put(&transaction, &question).await?;
        let envelope = changed(&transaction, database.node, question.session).await?;
        let publication = publication.lock().await;
        if stop.is_cancelled() {
            return Err(Fault::new(ErrorCode::Cancelled, "question was cancelled"));
        }
        crate::store::database::conflict::commit(transaction).await?;
        drop(write.take());
        database.questions.lock().unwrap().remove(&question.id);
        publication.send(events, envelope);
        database.feeds.publish(
            database.node,
            question.session,
            Change::Question(question.clone()),
        );
        Ok(question)
    }
    .await;
    if let Some(write) = write {
        if let Err(error) = crate::store::database::conflict::rollback(&database.connection).await {
            database.connection.hold_write(write);
            return Err(error);
        }
        drop(write);
    }
    result
}
