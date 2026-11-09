//! Thin, typed client for the private iCloud web services that icloud.com uses.
//!
//! Only two hosts families are ever contacted: `*.icloud.com` (API) and the
//! `*.icloud-content.com` download URLs that Apple hands back. Credentials are
//! the session cookies of the login web view; the password never passes here.

use parking_lot::RwLock;
use reqwest::{cookie::Jar, header, Client, Response, StatusCode};
use serde::Serialize;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};
use url::Url;

pub const ORIGIN: &str = "https://www.icloud.com";
pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/19.0 Safari/605.1.15";
pub const DEFAULT_ZONE: &str = "com.apple.CloudDocs";
pub const ROOT_ID: &str = "FOLDER::com.apple.CloudDocs::root";
const BUILD: &str = "2546Build54";
const PAGE: usize = 2000;

#[derive(thiserror::Error, Debug)]
pub enum ApiError {
    #[error("Your iCloud session expired. Sign in again.")]
    AuthExpired,
    #[error("You are not signed in.")]
    NotSignedIn,
    #[error("Network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Apple returned HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error("Unexpected response from Apple: {0}")]
    Shape(String),
    #[error("Disk error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Cancelled")]
    Cancelled,
}

impl Serialize for ApiError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub dsid: String,
    pub name: String,
    pub apple_id: String,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub account: Account,
    pub docws: String,
    pub drivews: String,
    pub client_id: String,
}

struct Transport {
    jar: Arc<Jar>,
    http: Client,
}

fn transport() -> Transport {
    let jar = Arc::new(Jar::default());
    let mut h = header::HeaderMap::new();
    h.insert(header::ORIGIN, header::HeaderValue::from_static(ORIGIN));
    h.insert(
        header::REFERER,
        header::HeaderValue::from_static("https://www.icloud.com/"),
    );
    h.insert(header::ACCEPT, header::HeaderValue::from_static("*/*"));
    let http = Client::builder()
        .cookie_provider(jar.clone())
        .user_agent(USER_AGENT)
        .default_headers(h)
        .gzip(true)
        .connect_timeout(Duration::from_secs(15))
        .pool_max_idle_per_host(32)
        .tcp_nodelay(true)
        .build()
        .expect("http client");
    Transport { jar, http }
}

pub struct Icloud {
    t: RwLock<Transport>,
    session: RwLock<Option<Session>>,
}

/// A cookie as read from the login web view.
pub struct RawCookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
}

async fn check(resp: Response) -> ApiResult<Response> {
    let status = resp.status();
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) || status.as_u16() == 421
    {
        return Err(ApiError::AuthExpired);
    }
    if !status.is_success() {
        let mut body = resp.text().await.unwrap_or_default();
        body.truncate(300);
        return Err(ApiError::Status {
            status: status.as_u16(),
            body,
        });
    }
    Ok(resp)
}

fn str_of(v: &Value) -> Option<String> {
    match v {
        Value::String(x) if !x.is_empty() => Some(x.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn host_url(u: &str) -> String {
    u.trim_end_matches('/').replace(":443", "")
}

impl Icloud {
    pub fn new() -> Self {
        Self {
            t: RwLock::new(transport()),
            session: RwLock::new(None),
        }
    }

    fn http(&self) -> Client {
        self.t.read().http.clone()
    }

    pub fn session(&self) -> ApiResult<Session> {
        self.session.read().clone().ok_or(ApiError::NotSignedIn)
    }

    pub fn account(&self) -> Option<Account> {
        self.session.read().as_ref().map(|s| s.account.clone())
    }

    pub fn sign_out(&self) {
        *self.t.write() = transport();
        *self.session.write() = None;
    }

    /// Replace the cookie jar with the cookies of the login web view.
    pub fn load_cookies(&self, cookies: &[RawCookie]) -> usize {
        let fresh = transport();
        let mut n = 0;
        for c in cookies {
            let d = c.domain.trim_start_matches('.');
            if !(d.ends_with("icloud.com") || d.ends_with("apple.com")) {
                continue;
            }
            let Ok(url) = Url::parse(&format!("https://{d}/")) else {
                continue;
            };
            let path = if c.path.is_empty() { "/" } else { &c.path };
            fresh.jar.add_cookie_str(
                &format!(
                    "{}={}; Domain=.{}; Path={}; Secure",
                    c.name, c.value, d, path
                ),
                &url,
            );
            n += 1;
        }
        *self.t.write() = fresh;
        n
    }

    fn params(s: &Session) -> [(&'static str, String); 4] {
        [
            ("clientBuildNumber", BUILD.to_string()),
            ("clientMasteringNumber", BUILD.to_string()),
            ("clientId", s.client_id.clone()),
            ("dsid", s.account.dsid.clone()),
        ]
    }

    /// Check the cookies with Apple and learn the account's service hosts.
    pub async fn validate(&self) -> ApiResult<Account> {
        let client_id = uuid::Uuid::new_v4().to_string().to_uppercase();
        let resp = self
            .http()
            .post("https://setup.icloud.com/setup/ws/1/validate")
            .query(&[
                ("clientBuildNumber", BUILD),
                ("clientMasteringNumber", BUILD),
                ("clientId", &client_id),
            ])
            .header(header::CONTENT_TYPE, "text/plain")
            .body("null")
            .timeout(Duration::from_secs(30))
            .send()
            .await?;
        let v: Value = check(resp).await?.json().await?;
        let ds = &v["dsInfo"];
        let dsid =
            str_of(&ds["dsid"]).ok_or_else(|| ApiError::Shape("validate: no dsid".into()))?;
        let ws = &v["webservices"];
        let drivews = str_of(&ws["drivews"]["url"]).map(|u| host_url(&u));
        let docws = str_of(&ws["docws"]["url"])
            .map(|u| host_url(&u))
            .or_else(|| drivews.as_ref().map(|d| d.replace("-drivews.", "-docws.")));
        let (Some(drivews), Some(docws)) = (drivews, docws) else {
            return Err(ApiError::Shape(
                "validate: iCloud Drive is not enabled for this account".into(),
            ));
        };
        let name = str_of(&ds["fullName"])
            .or_else(|| {
                let f = str_of(&ds["firstName"]).unwrap_or_default();
                let l = str_of(&ds["lastName"]).unwrap_or_default();
                let n = format!("{f} {l}").trim().to_string();
                (!n.is_empty()).then_some(n)
            })
            .unwrap_or_else(|| "iCloud account".into());
        let apple_id = str_of(&ds["appleId"])
            .or_else(|| str_of(&ds["primaryEmail"]))
            .unwrap_or_default();
        let account = Account {
            dsid,
            name,
            apple_id,
        };
        *self.session.write() = Some(Session {
            account: account.clone(),
            docws,
            drivews,
            client_id,
        });
        Ok(account)
    }

    /// One page of the Data Recovery list. Returns the page's records and the marker for
    /// the next page (None when this was the last page).
    pub async fn tombstone_page(
        &self,
        marker: Option<&str>,
    ) -> ApiResult<(Vec<Value>, Option<String>)> {
        let s = self.session()?;
        let mut q: Vec<(&str, String)> = Self::params(&s).to_vec();
        q.push(("limit", PAGE.to_string()));
        q.push(("unified_format", "true".into()));
        if let Some(m) = marker {
            q.push(("nextPage", m.to_string()));
        }
        // Apple's tombstone list is slow and flaky; a single failed page must not lose the scan.
        let mut attempt = 0u32;
        let mut v: Value = loop {
            attempt += 1;
            let res = async {
                let resp = self
                    .http()
                    .get(format!("{}/ws/_all_/list/enumerate/tombstones", s.docws))
                    .query(&q)
                    .timeout(Duration::from_secs(300))
                    .send()
                    .await?;
                Ok::<Value, ApiError>(check(resp).await?.json().await?)
            }
            .await;
            match res {
                Ok(v) => break v,
                Err(ApiError::AuthExpired) => return Err(ApiError::AuthExpired),
                Err(e) if attempt >= 6 => return Err(e),
                Err(e) => {
                    tracing::warn!(target: "scan", attempt, error = %e, "tombstone page failed, retrying");
                    tokio::time::sleep(Duration::from_secs(2u64 << attempt.min(5))).await;
                }
            }
        };
        let docs = match v.get_mut("documents").map(Value::take) {
            Some(Value::Array(d)) => d,
            _ => Vec::new(),
        };
        let next = str_of(&v["continuationMarker"])
            .filter(|_| v["status"].as_str() == Some("MORE_AVAILABLE"));
        Ok((docs, next))
    }

    /// Put deleted items back where they were. Returns `(item_id, error)` for failures.
    pub async fn recover(&self, ids: &[String]) -> ApiResult<Vec<(String, String)>> {
        let s = self.session()?;
        let body =
            json!({ "drive_item_update_request": { "is_recover": "true" }, "item_ids": ids });
        let resp = self
            .http()
            .put(format!("{}/v1/items", s.docws))
            .query(&Self::params(&s))
            .header(header::CONTENT_TYPE, "text/plain")
            .body(body.to_string())
            .timeout(Duration::from_secs(120))
            .send()
            .await?;
        let v: Value = check(resp).await?.json().await?;
        let mut failed = Vec::new();
        if let Some(list) = v["drive_items_with_status"].as_array() {
            let per_item = list
                .iter()
                .any(|e| e.get("item_id").is_some() || e.get("drive_item").is_some());
            for (i, e) in list.iter().enumerate() {
                let code = str_of(&e["status_code"]).unwrap_or_else(|| "200".into());
                if code == "200" {
                    continue;
                }
                let msg = str_of(&e["status_message"]).unwrap_or_else(|| format!("status {code}"));
                if per_item {
                    let id = str_of(&e["item_id"])
                        .or_else(|| str_of(&e["drive_item"]["item_id"]))
                        .or_else(|| ids.get(i).cloned())
                        .unwrap_or_default();
                    failed.push((id, msg));
                } else {
                    return Ok(ids.iter().map(|id| (id.clone(), msg.clone())).collect());
                }
            }
        }
        Ok(failed)
    }

    /// Folder metadata and its direct children (drivews format).
    pub async fn folders(&self, drivewsids: &[String], hierarchy: bool) -> ApiResult<Vec<Value>> {
        let s = self.session()?;
        let body: Vec<Value> = drivewsids
            .iter()
            .map(|id| json!({ "drivewsid": id, "partialData": false, "includeHierarchy": hierarchy }))
            .collect();
        let resp = self
            .http()
            .post(format!("{}/retrieveItemDetailsInFolders", s.drivews))
            .query(&Self::params(&s))
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Value::Array(body).to_string())
            .timeout(Duration::from_secs(60))
            .send()
            .await?;
        let v: Value = check(resp).await?.json().await?;
        Ok(match v {
            Value::Array(a) => a,
            _ => Vec::new(),
        })
    }

    pub async fn create_folder(&self, parent: &str, name: &str) -> ApiResult<String> {
        let s = self.session()?;
        let body = json!({
            "destinationDrivewsId": parent,
            "folders": [{ "clientId": format!("FOLDER::UNKNOWN_ZONE::TempId-{}", uuid::Uuid::new_v4()), "name": name }]
        });
        let resp = self
            .http()
            .post(format!("{}/createFolders", s.drivews))
            .query(&Self::params(&s))
            .header(header::CONTENT_TYPE, "text/plain")
            .body(body.to_string())
            .timeout(Duration::from_secs(60))
            .send()
            .await?;
        let v: Value = check(resp).await?.json().await?;
        let f = &v["folders"][0];
        match (str_of(&f["drivewsid"]), f["status"].as_str()) {
            (Some(id), Some("OK") | None) => Ok(id),
            _ => Err(ApiError::Shape(format!(
                "could not create folder \"{name}\""
            ))),
        }
    }

    /// Move items into a folder. Handles etag conflicts by retrying with Apple's etag.
    /// Returns `(drivewsid, error)` for failures.
    pub async fn move_items(
        &self,
        dest: &str,
        items: &[(String, String)],
    ) -> ApiResult<Vec<(String, String)>> {
        let s = self.session()?;
        let mut pending: Vec<(String, String)> = items.to_vec();
        let mut failed = Vec::new();
        for attempt in 0..3 {
            if pending.is_empty() {
                break;
            }
            let body = json!({
                "destinationDrivewsId": dest,
                "items": pending.iter().map(|(id, etag)| json!({ "drivewsid": id, "etag": etag, "clientId": id })).collect::<Vec<_>>()
            });
            let resp = self
                .http()
                .post(format!("{}/moveItems", s.drivews))
                .query(&Self::params(&s))
                .header(header::CONTENT_TYPE, "text/plain")
                .body(body.to_string())
                .timeout(Duration::from_secs(90))
                .send()
                .await?;
            let mut v: Value = check(resp).await?.json().await?;
            let results = match v.get_mut("items").map(Value::take) {
                Some(Value::Array(a)) => a,
                _ => Vec::new(),
            };
            let mut retry = Vec::new();
            for (i, r) in results.iter().enumerate() {
                let id = str_of(&r["drivewsid"])
                    .or_else(|| pending.get(i).map(|p| p.0.clone()))
                    .unwrap_or_default();
                match r["status"].as_str() {
                    Some("OK") | None => {}
                    Some("ETAG_CONFLICT") if attempt < 2 => {
                        retry.push((id, str_of(&r["etag"]).unwrap_or_default()));
                    }
                    Some(other) => failed.push((id, format!("move failed: {other}"))),
                }
            }
            pending = retry;
        }
        failed.extend(
            pending
                .into_iter()
                .map(|(id, _)| (id, "move failed: etag conflict".into())),
        );
        Ok(failed)
    }

    pub async fn download_url(&self, zone: &str, document_id: &str) -> ApiResult<String> {
        let s = self.session()?;
        let zone = if zone.is_empty() { DEFAULT_ZONE } else { zone };
        let mut q: Vec<(&str, String)> = Self::params(&s).to_vec();
        q.push(("document_id", document_id.to_string()));
        let resp = self
            .http()
            .get(format!("{}/ws/{}/download/by_id", s.docws, zone))
            .query(&q)
            .timeout(Duration::from_secs(60))
            .send()
            .await?;
        let v: Value = check(resp).await?.json().await?;
        str_of(&v["data_token"]["url"])
            .or_else(|| str_of(&v["package_token"]["url"]))
            .ok_or_else(|| ApiError::Shape("no download URL for this file".into()))
    }

    pub async fn download(&self, url: &str) -> ApiResult<Response> {
        let mut url = url.to_string();
        for _ in 0..5 {
            let resp = self.http().get(&url).send().await?;
            if resp.status().as_u16() == 330 {
                if let Some(loc) = resp
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|l| l.to_str().ok())
                {
                    url = loc.to_string();
                    continue;
                }
            }
            return check(resp).await;
        }
        Err(ApiError::Shape("too many redirects".into()))
    }
}
