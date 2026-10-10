//! Declarative enablement bindings connect packages to reusable Node services.
use super::*;
use plugin::Service;

fn binding(info: &Info) -> Option<Service> {
    info.extension.as_ref()?.enabled_service
}

pub(super) fn initial(db: &Connection, info: &Info) -> Result<Option<bool>, Fault> {
    match binding(info) {
        Some(Service::Language) => Ok(Some(super::super::code::settings(db)?.enabled)),
        None => Ok(None),
    }
}

pub(in crate::store) fn enabled(db: &Connection, service: Service) -> Result<Option<bool>, Fault> {
    if let Some(name) = owner(db, service, true)? {
        return Ok(Some(super::required(db, &name)?.summary.enabled));
    }
    Ok(owner(db, service, false)?.map(|_| false))
}

pub(super) fn apply(db: &Connection, info: &Info, enabled: bool) -> Result<(), Fault> {
    match binding(info) {
        Some(Service::Language) => super::super::code::enable(db, enabled),
        None => Ok(()),
    }
}

pub(super) fn check(db: &Connection, info: &Info) -> Result<(), Fault> {
    let Some(service) = binding(info) else {
        return Ok(());
    };
    if owner(db, service, true)?.is_some_and(|name| name != info.summary.name) {
        return Err(Fault::invalid(
            "Node service already has an installed package binding",
        ));
    }
    Ok(())
}

fn owner(db: &Connection, service: Service, installed: bool) -> Result<Option<String>, Fault> {
    let mut query = db
        .prepare("SELECT body FROM plugins WHERE installed=?1 ORDER BY name")
        .map_err(storage_error)?;
    for row in query
        .query_map([installed], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
    {
        let info: Info =
            serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error)?;
        if binding(&info) == Some(service) {
            return Ok(Some(info.summary.name));
        }
    }
    Ok(None)
}

pub(in crate::store) fn synchronize(
    db: &Connection,
    service: Service,
    enabled: bool,
) -> Result<Option<Summary>, Fault> {
    let name = owner(db, service, true)?.or(owner(db, service, false)?);
    match name {
        Some(name) => super::synchronize(db, &name, enabled),
        None => Ok(None),
    }
}
