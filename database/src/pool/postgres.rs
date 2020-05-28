use crate::pool::{Connection, ConnectionManager, ManagedConnection, Transaction};
use anyhow::Context as _;
use native_tls::{Certificate, TlsConnector};
use postgres_native_tls::MakeTlsConnector;
use std::convert::TryFrom;
use std::time::Duration;
use tokio_postgres::GenericClient as _;
use tokio_postgres::Statement;

pub struct Postgres(String);

impl Postgres {
    pub fn new(url: String) -> Self {
        Postgres(url)
    }
}

const CERT_URL: &str = "https://s3.amazonaws.com/rds-downloads/rds-ca-2019-root.pem";

lazy_static::lazy_static! {
    static ref CERTIFICATE_PEM: Vec<u8> = {
        let client = reqwest::blocking::Client::new();
        let resp = client
            .get(CERT_URL)
            .send()
            .expect("failed to get RDS cert");
         resp.bytes().expect("failed to get RDS cert body").to_vec()
    };
}

async fn make_client(db_url: &str) -> anyhow::Result<tokio_postgres::Client> {
    if db_url.contains("rds.amazonaws.com") {
        let cert = &CERTIFICATE_PEM[..];
        let cert = Certificate::from_pem(&cert).context("made certificate")?;
        let connector = TlsConnector::builder()
            .add_root_certificate(cert)
            .build()
            .context("built TlsConnector")?;
        let connector = MakeTlsConnector::new(connector);

        let (db_client, connection) = match tokio_postgres::connect(&db_url, connector).await {
            Ok(v) => v,
            Err(e) => {
                anyhow::bail!("failed to connect to DB: {}", e);
            }
        };
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("database connection error: {}", e);
            }
        });

        Ok(db_client)
    } else {
        eprintln!("Warning: Non-TLS connection to non-RDS DB");
        let (db_client, connection) =
            match tokio_postgres::connect(&db_url, tokio_postgres::NoTls).await {
                Ok(v) => v,
                Err(e) => {
                    anyhow::bail!("failed to connect to DB: {}", e);
                }
            };
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("database connection error: {}", e);
            }
        });

        Ok(db_client)
    }
}

#[async_trait::async_trait]
impl ConnectionManager for Postgres {
    type Connection = tokio_postgres::Client;
    async fn open(&self) -> Self::Connection {
        make_client(&self.0).await.unwrap()
    }
    async fn is_valid(&self, conn: &mut Self::Connection) -> bool {
        !conn.is_closed()
    }
}

#[async_trait::async_trait]
impl<'a> Transaction for PostgresTransaction<'a> {
    async fn commit(self: Box<Self>) -> Result<(), anyhow::Error> {
        Ok(self.conn.commit().await?)
    }
    async fn finish(self: Box<Self>) -> Result<(), anyhow::Error> {
        Ok(self.conn.rollback().await?)
    }
    fn conn(&mut self) -> &mut dyn Connection {
        self
    }
}

#[derive(Clone)]
pub struct CachedStatements {
    get_pstat: Statement,
    insert_pstat: Statement,
    get_self_profile_query: Statement,
    insert_self_profile_query: Statement,
    get_error: Statement,
    insert_error: Statement,
}

pub struct PostgresTransaction<'a> {
    statements: CachedStatements,
    conn: tokio_postgres::Transaction<'a>,
}

pub struct PostgresConnection {
    statements: CachedStatements,
    conn: ManagedConnection<tokio_postgres::Client>,
}

pub trait PClient {
    type Client: Send + Sync + tokio_postgres::GenericClient;
    fn conn(&self) -> &Self::Client;
    fn conn_mut(&mut self) -> &mut Self::Client;
    fn statements(&self) -> &CachedStatements;
}

impl<'a> PClient for PostgresTransaction<'a> {
    type Client = tokio_postgres::Transaction<'a>;
    fn conn(&self) -> &Self::Client {
        &self.conn
    }
    fn conn_mut(&mut self) -> &mut Self::Client {
        &mut self.conn
    }
    fn statements(&self) -> &CachedStatements {
        &self.statements
    }
}

impl PClient for PostgresConnection {
    type Client = tokio_postgres::Client;
    fn conn(&self) -> &Self::Client {
        &*self.conn
    }
    fn conn_mut(&mut self) -> &mut Self::Client {
        &mut *self.conn
    }
    fn statements(&self) -> &CachedStatements {
        &self.statements
    }
}

impl PostgresConnection {
    pub async fn new(conn: ManagedConnection<tokio_postgres::Client>) -> Self {
        PostgresConnection {
            statements: CachedStatements {
                get_pstat: conn
                    .prepare("select min(value) from pstat where series = $1 and cid $2;")
                    .await
                    .unwrap(),
                insert_pstat: conn
                    .prepare("insert into pstat(series, cid, value) VALUES ($1, $2, $3)")
                    .await
                    .unwrap(),
                get_self_profile_query: conn
                    .prepare(
                        "select
                        self_time, blocked_time, incremental_load_time, number_of_cache_hits, invocation_count
                        from self_profile_query
                        where series = $1 and cid = $2 order by self_time asc;
                        ",
                    )
                    .await
                    .unwrap(),
                insert_self_profile_query: conn
                    .prepare(
                        "insert into self_profile_query(
                            series,
                            cid,
                            self_time,
                            blocked_time,
                            incremental_load_time,
                            number_of_cache_hits,
                            invocation_count
                        ) VALUES ($1, $2, $3, $4, $5, $6, $7)",
                    )
                    .await
                    .unwrap(),
                get_error: conn.prepare("select value from errors where series = $1 and cid = $2;").await.unwrap(),
                insert_error: conn.prepare("insert into errors(series, cid, value) VALUES ($1, $2, $3)").await.unwrap(),
            },
            conn,
        }
    }
}

#[async_trait::async_trait]
impl<P> Connection for P
where
    P: Send + Sync + PClient,
{
    async fn maybe_create_tables(&mut self) {
        self.conn()
            .execute(
                "create table if not exists interned(name text primary key, value blob);",
                &[],
            )
            .await
            .unwrap();
        self.conn()
            .execute(
                "create table if not exists errors(series integer, cid integer, value text);",
                &[],
            )
            .await
            .unwrap();
        self.conn()
            .execute(
                "create table if not exists pstat(series integer, cid integer, value real);",
                &[],
            )
            .await
            .unwrap();
        self.conn()
            .execute(
                "create table if not exists self_profile_query(
                    series integer,
                    cid integer,
                    self_time integer,
                    blocked_time integer,
                    incremental_load_time integer,
                    number_of_cache_hits integer,
                    invocation_count integer
                );",
                &[],
            )
            .await
            .unwrap();
    }
    async fn maybe_create_indices(&mut self) {
        self.conn()
            .execute(
                "create index if not exists pstat_on_series_cid on pstat(series, cid);",
                &[],
            )
            .await
            .unwrap();
        self.conn().execute("create index if not exists self_profile_query_on_series_cid on self_profile_query(series, cid);", &[]).await.unwrap();
    }
    async fn transaction(&mut self) -> Box<dyn Transaction + '_> {
        let statements = self.statements().clone();
        let tx = self.conn_mut().transaction().await.unwrap();
        Box::new(PostgresTransaction {
            statements,
            conn: tx,
        })
    }
    async fn load_index(&mut self) -> Option<Vec<u8>> {
        let row = self
            .conn()
            .query_opt("select value from interned where name = 'index'", &[])
            .await
            .unwrap();
        match row {
            Some(r) => Some(r.get(0)),
            None => None,
        }
    }
    async fn store_index(&mut self, index: &[u8]) {
        self.conn()
            .execute(
                "insert or replace into interned (name, value) VALUES ('index', $1)",
                &[&index],
            )
            .await
            .unwrap();
    }
    async fn get_pstat(&mut self, series: u32, cid: crate::CollectionIdNumber) -> Option<f64> {
        self.conn()
            .query_opt(&self.statements().get_pstat, &[&series, &cid.pack()])
            .await
            .unwrap()
            .map(|r| r.get(0))
    }
    async fn insert_pstat(&mut self, series: u32, cid: crate::CollectionIdNumber, stat: f64) {
        self.conn()
            .execute(
                &self.statements().insert_pstat,
                &[&series, &cid.pack(), &stat],
            )
            .await
            .unwrap();
    }
    async fn get_self_profile_query(
        &mut self,
        series: u32,
        cid: crate::CollectionIdNumber,
    ) -> Option<crate::QueryDatum> {
        let row = self
            .conn()
            .query_opt(&self.statements().get_pstat, &[&series, &cid.pack()])
            .await
            .unwrap()?;
        let self_time: i64 = row.get(0);
        let blocked_time: i64 = row.get(1);
        let incremental_load_time: i64 = row.get(2);
        Some(crate::QueryDatum {
            self_time: Duration::from_nanos(self_time as u64),
            blocked_time: Duration::from_nanos(blocked_time as u64),
            incremental_load_time: Duration::from_nanos(incremental_load_time as u64),
            number_of_cache_hits: row.get(3),
            invocation_count: row.get(4),
        })
    }
    async fn insert_self_profile_query(
        &mut self,
        series: u32,
        cid: crate::CollectionIdNumber,
        data: &crate::QueryDatum,
    ) {
        self.conn()
            .execute(
                &self.statements().insert_self_profile_query,
                &[
                    &series,
                    &cid.pack(),
                    &i64::try_from(data.self_time.as_nanos()).unwrap(),
                    &i64::try_from(data.blocked_time.as_nanos()).unwrap(),
                    &i64::try_from(data.incremental_load_time.as_nanos()).unwrap(),
                    &data.number_of_cache_hits,
                    &data.invocation_count,
                ],
            )
            .await
            .unwrap();
    }
    async fn get_error(&mut self, series: u32, cid: crate::CollectionIdNumber) -> Option<String> {
        self.conn()
            .query_opt(&self.statements().get_error, &[&series, &cid.pack()])
            .await
            .unwrap()
            .map(|r| r.get(0))
    }
    async fn insert_error(&mut self, series: u32, cid: crate::CollectionIdNumber, text: &str) {
        self.conn()
            .execute(
                &self.statements().insert_error,
                &[&series, &cid.pack(), &text],
            )
            .await
            .unwrap();
    }
}