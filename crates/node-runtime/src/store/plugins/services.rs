//! Declarative enablement bindings connect packages to reusable Node services.
use super::*;
use plugin::Service;

fn binding(info: &Info) -> Option<Service> {
    info.extension
        .as_ref()?
        .enabled_service
        .map(|service| match service {
            Service::Language => Service::CodeIndex,
            service => service,
        })
}

pub(super) async fn initial(db: &Connection, info: &Info) -> Result<Option<bool>, Fault> {
    match binding(info) {
        Some(Service::CodeIndex) => Ok(Some(super::super::code_index::settings(db).await?.enabled)),
        None | Some(Service::Language) => Ok(None),
    }
}

pub(in crate::store) async fn enabled(
    db: &Connection,
    service: Service,
) -> Result<Option<bool>, Fault> {
    if let Some(name) = owner(db, service, true).await? {
        return Ok(Some(super::required(db, &name).await?.summary.enabled));
    }
    Ok(owner(db, service, false).await?.map(|_| false))
}

pub(super) async fn apply(db: &Connection, info: &Info, enabled: bool) -> Result<(), Fault> {
    match binding(info) {
        Some(Service::CodeIndex) => super::super::code_index::enable(db, enabled).await,
        None | Some(Service::Language) => Ok(()),
    }
}

pub(super) async fn check(db: &Connection, info: &Info) -> Result<(), Fault> {
    let Some(service) = binding(info) else {
        return Ok(());
    };
    if owner(db, service, true)
        .await?
        .is_some_and(|name| name != info.summary.name)
    {
        return Err(Fault::invalid(
            "Node service already has an installed package binding",
        ));
    }
    Ok(())
}

async fn owner(
    db: &Connection,
    service: Service,
    installed: bool,
) -> Result<Option<String>, Fault> {
    let mut query = db
        .prepare("SELECT body FROM plugins WHERE installed=?1 ORDER BY name")
        .await
        .map_err(storage_error)?;
    {
        let mut rows = query.query([installed]).await.map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let row: turso::Result<_> = row.get::<Vec<u8>>(0);

            let info: Info =
                serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error)?;
            if binding(&info) == Some(service) {
                return Ok(Some(info.summary.name));
            }
        }
    }
    Ok(None)
}

pub(in crate::store) async fn synchronize(
    db: &Connection,
    service: Service,
    enabled: bool,
) -> Result<Option<Summary>, Fault> {
    let name = owner(db, service, true)
        .await?
        .or(owner(db, service, false).await?);
    match name {
        Some(name) => super::synchronize(db, &name, enabled).await,
        None => Ok(None),
    }
}
