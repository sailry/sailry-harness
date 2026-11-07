use super::{Fetcher, SocketAddr, policy, proxy, unavailable};
use reqwest::{Client, Url, dns};
use sailry_protocol::Fault;
use std::{collections::VecDeque, time::Duration};

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(in crate::web::fetch) struct Clients(VecDeque<(Key, Client)>);

#[derive(PartialEq, Eq)]
struct Key {
    proxy: Option<String>,
    #[cfg(any(test, feature = "test-support"))]
    trust: Option<String>,
}

impl Clients {
    pub(super) fn get(
        &mut self,
        owner: &Fetcher,
        url: &Url,
        proxy: Option<proxy::Selected>,
    ) -> Result<Client, Fault> {
        let key = Key {
            proxy: proxy.as_ref().map(|selected| selected.key.clone()),
            #[cfg(any(test, feature = "test-support"))]
            trust: owner
                .fixtures
                .lock()
                .expect("web fixture lock is healthy")
                .get(url.host_str().expect("validated URL host"))
                .filter(|fixture| !fixture.roots.is_empty())
                .map(|_| url.host_str().unwrap().to_owned()),
        };
        if let Some(index) = self.0.iter().position(|(existing, _)| existing == &key) {
            let entry = self.0.remove(index).unwrap();
            let client = entry.1.clone();
            self.0.push_back(entry);
            return Ok(client);
        }
        let builder = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .gzip(true)
            .brotli(true)
            .no_deflate()
            .no_zstd()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(10));
        let builder = if let Some(selected) = proxy {
            builder.proxy(selected.proxy)
        } else {
            builder.dns_resolver(Resolver::new(owner))
        };
        #[cfg(any(test, feature = "test-support"))]
        let builder = owner.configure_fixture(url, builder);
        #[cfg(not(any(test, feature = "test-support")))]
        let _ = url;
        let client = builder
            .build()
            .map_err(|_| unavailable("web HTTP client is unavailable"))?;
        if self.0.len() == 16 {
            self.0.pop_front();
        }
        self.0.push_back((key, client.clone()));
        Ok(client)
    }
}

#[derive(Clone)]
pub(super) struct Resolver {
    #[cfg(any(test, feature = "test-support"))]
    fixtures:
        std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<String, super::super::Fixture>>>,
}

impl Resolver {
    pub(super) fn new(_owner: &Fetcher) -> Self {
        Self {
            #[cfg(any(test, feature = "test-support"))]
            fixtures: _owner.fixtures.clone(),
        }
    }

    pub(super) async fn lookup(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, Fault> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(fixture) = self
            .fixtures
            .lock()
            .expect("web fixture lock is healthy")
            .get(host)
            .cloned()
        {
            if fixture.resolved.is_empty() {
                return Err(unavailable("web hostname could not be resolved"));
            }
            return Ok(fixture
                .resolved
                .into_iter()
                .map(|ip| SocketAddr::new(ip, port))
                .collect());
        }
        tokio::net::lookup_host((host.trim_matches(['[', ']']), port))
            .await
            .map(|addresses| addresses.collect())
            .map_err(|_| unavailable("web hostname could not be resolved"))
    }

    pub(super) async fn checked(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, Fault> {
        let addresses = self.lookup(host, port).await?;
        policy::addresses(&addresses, &super::local_addresses().await?)?;
        #[cfg(any(test, feature = "test-support"))]
        if let Some(fixture) = self
            .fixtures
            .lock()
            .expect("web fixture lock is healthy")
            .get(host)
        {
            return Ok(vec![fixture.address]);
        }
        Ok(addresses)
    }
}

impl dns::Resolve for Resolver {
    fn resolve(&self, name: dns::Name) -> dns::Resolving {
        let resolver = self.clone();
        Box::pin(async move {
            let addresses = resolver
                .checked(name.as_str(), 0)
                .await
                .map_err(|fault| std::io::Error::other(fault.message))?;
            Ok(Box::new(addresses.into_iter()) as dns::Addrs)
        })
    }
}
