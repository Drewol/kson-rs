use reqwest::{
    blocking::Body,
    header::{HeaderMap, HeaderName, HeaderValue},
    Method,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, str::FromStr};

use tealr::{
    mlu::{
        mlua::{Function, Lua, RegistryKey},
        ExportInstances, FromToLua, TealData, UserData, UserDataProxy,
    },
    ToTypename, TypeName,
};

#[derive(Default)]
pub struct LuaHttp {
    calls: Vec<poll_promise::Promise<Response>>,
    callbacks: HashMap<i64, RegistryKey>,
    next_id: i64,
}

#[derive(Debug, Serialize, Deserialize, FromToLua, ToTypename, Clone)]
struct Response {
    #[serde(skip)]
    id: i64,
    url: String,
    text: String,
    status: i32,
    elapsed: f32,
    error: String,
    cookies: String,
    header: HashMap<String, String>,
}

impl Response {
    pub fn error(error: String) -> Self {
        Self {
            id: 0,
            url: String::new(),
            text: String::new(),
            status: -1,
            elapsed: 0.0,
            error,
            cookies: String::new(),
            header: HashMap::new(),
        }
    }

    pub async fn from_response(response: reqwest::Response) -> Self {
        Self {
            id: -1,
            url: response.url().to_string(),
            status: response.status().as_u16() as _,
            elapsed: 1.0,
            error: String::new(),
            cookies: String::new(),
            header: response
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_string(),
                        value.to_str().unwrap_or_default().to_string(),
                    )
                })
                .collect(),
            text: response.text().await.unwrap_or_default(),
        }
    }

    pub fn from_response_blocking(response: reqwest::blocking::Response) -> Self {
        Self {
            id: -1,
            url: response.url().to_string(),
            status: response.status().as_u16() as _,
            elapsed: 1.0,
            error: String::new(),
            cookies: String::new(),
            header: response
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_string(),
                        value.to_str().unwrap_or_default().to_string(),
                    )
                })
                .collect(),
            text: response.text().unwrap_or_default(),
        }
    }
}

impl LuaHttp {
    pub fn poll(lua: &Lua) {
        let (mut calls, mut callbacks) = {
            let mut http = lua.app_data_mut::<LuaHttp>().unwrap();

            (
                std::mem::take(&mut http.calls),
                std::mem::take(&mut http.callbacks),
            )
        };

        let mut remaining_calls = vec![];
        for ele in calls.drain(..) {
            match ele.try_take() {
                Ok(data) => {
                    if let Some(key) = callbacks.remove(&data.id) {
                        if let Ok(callback) = lua.registry_value::<Function>(&key) {
                            _ = callback.call::<_, ()>(data);
                        }
                    }
                }
                Err(call) => remaining_calls.push(call),
            }
        }

        {
            let mut http = lua.app_data_mut::<LuaHttp>().unwrap();
            http.calls.append(&mut remaining_calls);
            for (id, key) in callbacks.drain() {
                http.callbacks.insert(id, key);
            }
        }
    }
}

#[derive(Default, ToTypename, UserData)]
pub struct ExportLuaHttp;

impl TealData for ExportLuaHttp {
    fn add_methods<'lua, T: tealr::mlu::TealDataMethods<'lua, Self>>(methods: &mut T) {
        tealr::mlu::create_named_parameters!(GetParams with
            url : String,
            headers : HashMap<String, String>,
        );

        tealr::mlu::create_named_parameters!(PostParams with
            url : String,
            content: String,
            headers : HashMap<String, String>,
        );

        methods.add_function("Get", |_, GetParams { url, headers }: GetParams| {
            let mut req = reqwest::blocking::Request::new(
                Method::GET,
                reqwest::Url::parse(&url).map_err(tealr::mlu::mlua::Error::external)?,
            );

            for (header, value) in headers.iter() {
                req.headers_mut().append(
                    HeaderName::from_str(&header).map_err(tealr::mlu::mlua::Error::external)?,
                    HeaderValue::from_str(&value).map_err(tealr::mlu::mlua::Error::external)?,
                );
            }

            reqwest::blocking::Client::new()
                .execute(req)
                .map(Response::from_response_blocking)
                .map_err(tealr::mlu::mlua::Error::external)
        });

        methods.add_function(
            "Post",
            |_,
             PostParams {
                 url,
                 content,
                 headers,
             }| {
                let client = reqwest::blocking::Client::builder().build().unwrap();

                let mut req = client.post(url).body(content);
                for (header, value) in headers.iter() {
                    req = req.header(header, value);
                }

                let req = req.build().unwrap();

                client
                    .execute(req)
                    .map(Response::from_response_blocking)
                    .map_err(tealr::mlu::mlua::Error::external)
            },
        );

        methods.add_function(
            "GetAsync",
            |lua, (url, headers, callback): (String, HashMap<String, String>, Function<'lua>)| {
                if let Some(mut http) = lua.app_data_mut::<LuaHttp>() {
                    let id = http.next_id;
                    http.callbacks
                        .insert(id, lua.create_registry_value(callback)?);

                    http.calls
                        .push(poll_promise::Promise::spawn_async(async move {
                            let client = reqwest::Client::builder()
                                .default_headers(HeaderMap::from_iter(headers.iter().map(
                                    |(name, value)| (name.parse().unwrap(), value.parse().unwrap()),
                                )))
                                .build()
                                .unwrap();

                            let req = client.get(url).build().unwrap();

                            match client.execute(req).await.map(Response::from_response) {
                                Ok(mut r) => {
                                    let mut r = r.await;
                                    r.id = id;
                                    r
                                }
                                Err(e) => Response::error(format!("{:?}", e)),
                            }
                        }));

                    http.next_id += 1;
                }
                Ok(())
            },
        );

        methods.add_function(
            "PostAsync",
            |lua,
             (url, content, headers, callback): (
                String,
                String,
                HashMap<String, String>,
                Function<'lua>,
            )| {
                if let Some(mut http) = lua.app_data_mut::<LuaHttp>() {
                    let id = http.next_id;
                    http.callbacks
                        .insert(id, lua.create_registry_value(callback)?);

                    http.calls
                        .push(poll_promise::Promise::spawn_async(async move {
                            let client = reqwest::Client::builder()
                                .default_headers(HeaderMap::from_iter(headers.iter().map(
                                    |(name, value)| (name.parse().unwrap(), value.parse().unwrap()),
                                )))
                                .build()
                                .unwrap();

                            let request = client.post(url).body(content).build().unwrap();

                            match client.execute(request).await.map(Response::from_response) {
                                Ok(mut r) => {
                                    let mut r = r.await;
                                    r.id = id;
                                    r
                                }
                                Err(e) => Response::error(format!("{:?}", e)),
                            }
                        }));

                    http.next_id += 1;
                }
                Ok(())
            },
        )
    }

    fn add_fields<'lua, F: tealr::mlu::TealDataFields<'lua, Self>>(_fields: &mut F) {}
}

impl ExportInstances for ExportLuaHttp {
    fn add_instances<'lua, T: tealr::mlu::InstanceCollector<'lua>>(
        self,
        instance_collector: &mut T,
    ) -> tealr::mlu::mlua::Result<()> {
        instance_collector.add_instance("http", UserDataProxy::<ExportLuaHttp>::new)?;
        Ok(())
    }
}
