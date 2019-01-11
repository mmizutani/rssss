#[macro_use]
extern crate failure;
#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;

extern crate actix_web;
extern crate bytes;
extern crate futures;
extern crate listenfd;
extern crate scraper;
extern crate simple_logger;
extern crate xml;

pub mod error;
pub mod rss;

use actix_web::client::{ClientResponse, SendRequest};
use actix_web::middleware::cors::Cors;
use actix_web::{
    client, http, server, App, AsyncResponder, Error, HttpMessage, HttpResponse, Query,
};
use futures::future;
use futures::future::Future;
use listenfd::ListenFd;
use std::time::Duration;

#[derive(Deserialize)]
struct Info {
    url: String,
}

fn get_feed(info: Query<Info>) -> Box<Future<Item = HttpResponse, Error = Error>> {
    let url = &info.url;
    debug!("{}", url);
    send_request(url)
        .map_err(Error::from)
        .and_then(|r| retrieve_response(r, true))
        .responder()
}

fn send_request(url: &String) -> SendRequest {
    client::get(url)
        .header("User-Agent", "rssss")
        .timeout(Duration::from_secs(60))
        .finish()
        .unwrap()
        .send()
}

fn retrieve_response(
    res: ClientResponse,
    enable_redirect: bool,
) -> Box<Future<Item = HttpResponse, Error = Error>> {
    let status = res.status();
    if status.is_success() {
        Box::new(
            res.body()
                .limit(1_048_576) // 1MB
                .from_err()
                .and_then(|b| match rss::parse_rss(b) {
                    Ok(r) => Ok(HttpResponse::Ok().json(r)),
                    Err(e) => {
                        error!("{}", e);
                        Ok(HttpResponse::InternalServerError().finish())
                    }
                }),
        )
    } else if status.is_redirection() && enable_redirect {
        match res.headers().get("location") {
            Some(location) => match location.to_str() {
                Ok(url) => Box::new(
                    send_request(&url.to_string())
                        .map_err(Error::from)
                        .and_then(|r| retrieve_response(r, false)),
                ),
                _ => Box::new(future::ok::<HttpResponse, Error>(
                    HttpResponse::InternalServerError().finish(),
                )),
            },
            _ => Box::new(future::ok::<HttpResponse, Error>(
                HttpResponse::InternalServerError().finish(),
            )),
        }
    } else {
        warn!("Invalid status: {}", res.status());
        Box::new(future::ok::<HttpResponse, Error>(
            HttpResponse::build(res.status()).finish(),
        ))
    }
}

fn main() {
    simple_logger::init_with_level(log::Level::Info).unwrap();

    let mut listenfd = ListenFd::from_env();

    let mut server = server::new(|| {
        App::new().configure(|app| {
            Cors::for_app(app)
                .send_wildcard()
                .allowed_methods(vec!["GET", "POST"])
                .allowed_header(http::header::CONTENT_TYPE)
                .resource("/feed", |r| r.method(http::Method::GET).with(get_feed))
                .register()
        })
    });

    server = if let Some(l) = listenfd.take_tcp_listener(0).unwrap() {
        server.listen(l)
    } else {
        server.bind("127.0.0.1:8080").unwrap()
    };

    server.run();
}
