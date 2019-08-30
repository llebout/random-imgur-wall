#![recursion_limit = "8192"]

use failure::{format_err, Error};

use serde::{Deserialize, Serialize};

use yew::format::{Binary, Json, Nothing};

use yew::services::console::ConsoleService;
use yew::services::fetch::{
    FetchOptions, FetchService, FetchTask, Redirect, Referrer, ReferrerPolicy, Request, Response,
};
use yew::services::interval::{IntervalService, IntervalTask};
use yew::services::timeout::{TimeoutService, TimeoutTask};
use yew::services::websocket::{WebSocketService, WebSocketStatus, WebSocketTask};

use yew::{html, html::ChangeData, Component, ComponentLink, Html, Renderable, ShouldRender};

use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};

use std::iter;

use std::time::Duration;

use http::response::Parts;
use std::collections::HashMap;
use std::collections::VecDeque;

#[derive(Serialize, Deserialize)]
struct Config {
    ws_url: String,
}

#[derive(Serialize, Deserialize)]
enum WsMessageType {
    UsersBruteforcing,
    UsersWatching,
    Start,
    Stop,
    New,
}

#[derive(Serialize, Deserialize)]
struct WsMessage {
    msg_type: WsMessageType,
    text: Option<String>,
    number: Option<u64>,
}

struct Model {
    link: ComponentLink<Model>,
    config: Option<Config>,
    console_service: ConsoleService,
    fetch_service: FetchService,
    fetch_task: Option<FetchTask>,
    find_fetch_tasks: HashMap<String, FetchTask>,
    ws_service: WebSocketService,
    ws_task: Option<WebSocketTask>,
    interval_service: IntervalService,
    interval_task: Option<IntervalTask>,
    reset_interval_task: Option<IntervalTask>,
    rate_interval_task: Option<IntervalTask>,
    timeout_service: TimeoutService,
    timeout_task: Option<TimeoutTask>,
    is_started: bool,
    interval: Duration,
    images: VecDeque<String>,
    total_requests: u64,
    requests_per_second: u64,
    requests_per_second_current: u64,
    images_found_self: u64,
    images_found: u64,
    users_watching: u64,
    users_bruteforcing: u64,
    concurrent_loaded: usize,
    show_from_top: bool,
    is_rate_limited: bool,
    rate_limit: u64,
}

enum Msg {
    FetchConfig,
    FetchConfigDone(Result<Config, Error>),
    WsConnect,
    WsConnected,
    WsLost,
    WsMessage(Result<WsMessage, Error>),
    WsSend(WsMessage),
    IntervalChanged(String),
    Start,
    Stop,
    TryFind,
    Found((String, String)),
    NotFound(String),
    ResetRequestsPerSecond,
    LoadedChanged(String),
    ShowModeSelected(bool),
    RateLimitChanged(String),
    ResetRateLimit,
    NoOp,
}

impl Component for Model {
    type Message = Msg;
    type Properties = ();

    fn create(_: Self::Properties, mut link: ComponentLink<Self>) -> Self {
        let fetch_service = FetchService::new();
        let ws_service = WebSocketService::new();
        let interval_service = IntervalService::new();
        let console_service = ConsoleService::new();
        let timeout_service = TimeoutService::new();

        link.send_self(Msg::FetchConfig);

        Model {
            link,
            config: None,
            console_service,
            fetch_service,
            fetch_task: None,
            find_fetch_tasks: HashMap::new(),
            ws_service,
            ws_task: None,
            interval_service,
            interval_task: None,
            reset_interval_task: None,
            rate_interval_task: None,
            timeout_service,
            timeout_task: None,
            is_started: false,
            interval: Duration::from_millis(100),
            images: VecDeque::new(),
            total_requests: 0,
            requests_per_second: 0,
            requests_per_second_current: 0,
            images_found_self: 0,
            images_found: 0,
            users_watching: 0,
            users_bruteforcing: 0,
            concurrent_loaded: 100,
            show_from_top: true,
            is_rate_limited: true,
            rate_limit: 2,
        }
    }

    fn update(&mut self, msg: Self::Message) -> ShouldRender {
        match msg {
            Msg::FetchConfig => {
                self.fetch_task =
                    Some(self.fetch_service.fetch(
                        Request::get("/config.json").body(Nothing).unwrap(),
                        self.link.send_back(
                            move |response: Response<Json<Result<Config, Error>>>| {
                                let (meta, Json(config)) = response.into_parts();
                                if meta.status.is_success() {
                                    Msg::FetchConfigDone(config)
                                } else {
                                    Msg::FetchConfigDone(Err(format_err!(
                                        "{}: could not fetch /config.json",
                                        meta.status
                                    )))
                                }
                            },
                        ),
                    ));

                false
            }
            Msg::FetchConfigDone(Ok(config)) => {
                self.config = Some(config);

                self.link.send_self(Msg::WsConnect);
                self.reset_interval_task = Some(self.interval_service.spawn(
                    Duration::from_secs(1),
                    self.link.send_back(|_| Msg::ResetRequestsPerSecond),
                ));
                self.rate_interval_task = Some(self.interval_service.spawn(
                    Duration::from_secs(self.rate_limit),
                    self.link.send_back(|_| Msg::ResetRateLimit),
                ));

                false
            }
            Msg::WsConnect => {
                if let Some(config) = &self.config {
                    if self.ws_task.is_none() {
                        let callback = self.link.send_back(|Json(data)| Msg::WsMessage(data));
                        let notification = self.link.send_back(|status| match status {
                            WebSocketStatus::Opened => Msg::WsConnected,
                            WebSocketStatus::Closed | WebSocketStatus::Error => Msg::WsLost.into(),
                        });
                        let task = self
                            .ws_service
                            .connect(&config.ws_url, callback, notification);
                        self.ws_task = Some(task);
                    }
                }
                false
            }
            Msg::WsConnected => {
                self.link.send_self(Msg::Start);
                false
            }
            Msg::WsLost => {
                self.ws_task = None;

                self.timeout_task = Some(self.timeout_service.spawn(
                    Duration::from_secs(1),
                    self.link.send_back(|_| Msg::WsConnect),
                ));

                false
            }
            Msg::WsSend(msg) => {
                self.ws_task.as_mut().unwrap().send(Json(&msg));

                false
            }
            Msg::WsMessage(Ok(msg)) => match msg.msg_type {
                WsMessageType::New => {
                    if let Some(text) = msg.text {
                        if text.is_ascii() && text.chars().all(char::is_alphanumeric) {
                            if self.is_rate_limited == false || self.rate_limit == 0 {
                                if self.concurrent_loaded != 0 {
                                    while self.images.len() > self.concurrent_loaded {
                                        if self.show_from_top {
                                            self.images.pop_front();
                                        } else {
                                            self.images.pop_back();
                                        }
                                    }

                                    if self.images.len() >= self.concurrent_loaded {
                                        if self.show_from_top {
                                            self.images.pop_front();
                                        } else {
                                            self.images.pop_back();
                                        }
                                    }
                                }

                                if self.show_from_top {
                                    self.images.push_back(text);
                                } else {
                                    self.images.push_front(text);
                                }

                                self.is_rate_limited = true;
                            }

                            self.images_found += 1;

                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
                WsMessageType::UsersWatching => {
                    if let Some(number) = msg.number {
                        self.users_watching = number;
                        true
                    } else {
                        false
                    }
                }
                WsMessageType::UsersBruteforcing => {
                    if let Some(number) = msg.number {
                        self.users_bruteforcing = number;
                        true
                    } else {
                        false
                    }
                }
                _ => false,
            },
            Msg::TryFind => {
                let alnum = iter::repeat(())
                    .map(|()| thread_rng().sample(Alphanumeric))
                    .take(7)
                    .collect::<String>();

                self.find_fetch_tasks.insert(
                    alnum.to_owned(),
                    self.fetch_service.fetch_binary_with_options(
                        Request::get(format!("https://i.imgur.com/{}.png", &alnum))
                            .body(Nothing)
                            .unwrap(),
                        FetchOptions {
                            cache: None,
                            credentials: None,
                            redirect: Some(Redirect::Error),
                            mode: None,
                            referrer: None,
                            referrer_policy: Some(ReferrerPolicy::NoReferrer),
                            integrity: None,
                        },
                        self.link.send_back(move |response: Response<Nothing>| {
                            let (meta, _) = response.into_parts();

                            let message = format!("{:#?}", meta);

                            if meta.status.as_u16() != 408 {
                                Msg::Found((message, alnum.clone()))
                            } else {
                                Msg::NotFound(message)
                            }
                        }),
                    ),
                );

                false
            }
            Msg::Found((message, data)) => {
                // self.console_service.log(&message);

                self.find_fetch_tasks.remove(&data);
                self.link.send_self(Msg::WsSend(WsMessage {
                    msg_type: WsMessageType::New,
                    text: Some(data),
                    number: None,
                }));

                self.images_found_self += 1;
                self.requests_per_second_current += 1;
                self.total_requests += 1;

                true
            }
            Msg::NotFound(message) => {
                // self.console_service.log(&message);

                self.requests_per_second_current += 1;
                self.total_requests += 1;

                true
            }
            Msg::IntervalChanged(new_interval) => {
                if let Ok(interval) = new_interval.parse::<u64>() {
                    self.interval = Duration::from_millis(interval);
                }

                if self.is_started {
                    self.interval_task = Some(
                        self.interval_service
                            .spawn(self.interval, self.link.send_back(|_| Msg::TryFind)),
                    );
                }

                false
            }
            Msg::LoadedChanged(new_loaded) => {
                if let Ok(loaded) = new_loaded.parse::<usize>() {
                    self.concurrent_loaded = loaded;
                }

                false
            }
            Msg::ShowModeSelected(value) => {
                self.show_from_top = value;

                true
            }
            Msg::RateLimitChanged(new_rate_limit) => {
                if let Ok(rate_limit) = new_rate_limit.parse::<u64>() {
                    self.rate_limit = rate_limit;

                    if self.rate_limit != 0 {
                        self.rate_interval_task = Some(self.interval_service.spawn(
                            Duration::from_secs(self.rate_limit),
                            self.link.send_back(|_| Msg::ResetRateLimit),
                        ));
                    }
                }

                false
            }
            Msg::Start => {
                if self.is_started == false {
                    self.interval_task = Some(
                        self.interval_service
                            .spawn(self.interval, self.link.send_back(|_| Msg::TryFind)),
                    );

                    self.link.send_self(Msg::WsSend(WsMessage {
                        msg_type: WsMessageType::Start,
                        text: None,
                        number: None,
                    }));
                }

                self.is_started = true;

                false
            }
            Msg::Stop => {
                self.interval_task = None;

                if self.is_started == true {
                    self.link.send_self(Msg::WsSend(WsMessage {
                        msg_type: WsMessageType::Stop,
                        text: None,
                        number: None,
                    }));
                }

                self.is_started = false;

                false
            }
            Msg::ResetRequestsPerSecond => {
                self.requests_per_second = self.requests_per_second_current;
                self.requests_per_second_current = 0;

                true
            }
            Msg::ResetRateLimit => {
                self.is_rate_limited = false;

                false
            }
            _ => false,
        }
    }
}

impl Renderable<Model> for Model {
    fn view(&self) -> Html<Self> {
        html! {
            <body>
                <header>
                    <h1>{ "Random Imgur Wall" }</h1>
                </header>
                <main>
                    <div id="container">
                        <section id="info">
                            <h2>{ "NSFL Warning" }</h2>
                            <p>{ "Images show up randomly and you may see terrible things staying on this site, watch with care." }</p>
                            <p>
                                <a target="_blank" rel="noopener" referrerpolicy="no-referrer" href="https://help.imgur.com/hc/en-us/articles/208582296-Reporting-Content">
                                    { "Report abusive content" }
                                </a>
                            </p>
                            <p>
                                <a target="_blank" rel="noopener" referrerpolicy="no-referrer" href={ "https://github.com/leo-lb/random-imgur-wall" }>
                                    { "Source code" }
                                </a>
                            </p>
                            <p>
                                { "Thanks to " }
                                <a target="_blank" rel="noopener" referrerpolicy="no-referrer" href={ "https://www.reddit.com/user/quickscoperdoge" }>
                                    { "u/quickscoperdoge" }
                                </a>
                                { " for the revamped design!" }
                            </p>
                        </section>
                        <section id="settings">
                            <h2>{ "Settings" }</h2>
                            <table>
                                <tr>
                                    <td><label for="interval">{ "Interval at which bruteforce requests are sent (in ms)" }</label></td>
                                    <td><input id="interval" type="number" value=self.interval.as_millis() oninput=|e| Msg::IntervalChanged(e.value) /></td> // <!-- modify this -->
                                </tr>
                                <tr>
                                    <td><label for="images">{ "Number of images to keep loaded at a time (0 for unlimited)" }</label></td>
                                    <td><input id="images" type="number" value=self.concurrent_loaded oninput=|e| Msg::LoadedChanged(e.value) /></td> // <!-- modify this -->
                                </tr>
                                <tr>
                                    <td><label for="mode">{ "Show mode" }</label></td>
                                    <td><select id="mode" name="Show mode" onchange=|event| {
                                                                                match event {
                                                                                    ChangeData::Select(elem) => {
                                                                                        let value = elem.selected_index().map(|x| x as usize);
                                                                                        if let Some(idx) = value {
                                                                                            Msg::ShowModeSelected(idx != 1)
                                                                                        } else {
                                                                                            unreachable!();
                                                                                        }
                                                                                    }
                                                                                    _ => {
                                                                                        unreachable!();
                                                                                    }
                                                                                } }> // <!-- modify this -->
                                        <option>{ "Show from top" }</option>
                                        <option>{ "Show from bottom" }</option>
                                    </select></td>
                                </tr>
                                <tr>
                                    <td><label for="delay">{ "Delay to wait before a new image shows up (in seconds, 0 for none)" }</label></td>
                                    <td><input id="delay" type="number" value=self.rate_limit oninput=|e| Msg::RateLimitChanged(e.value) /></td> //<!-- modify this -->
                                </tr>
                            </table>
                            <p style="overflow: auto;">
                                <button type="button" style="margin: auto; width: 50%;" onclick=|_| Msg::Start>{ "Start" }</button> //<!-- modify this -->
                                <button type="button" style="margin: auto; width: 50%;" onclick=|_| Msg::Stop>{ "Stop" }</button> //<!-- modify this -->
                            </p>
                        </section>

                        <section id="stats">
                            <h2>{ "Statistics" }</h2>
                            <table>
                                <tr>
                                    <td>{ "Total number of requests" }</td>
                                    <td>{ self.total_requests }</td>
                                </tr>
                                <tr>
                                    <td>{ "Requests completed per second" }</td>
                                    <td>{ self.requests_per_second }</td>
                                </tr>
                                <tr>
                                    <td>{ "Images you found" }</td>
                                    <td>{ self.images_found_self }</td>
                                </tr>
                                <tr>
                                    <td>{ "Images everyone found" }</td>
                                    <td>{ self.images_found }</td>
                                </tr>
                                <tr>
                                    <td>{ "Users watching" }</td>
                                    <td>{ self.users_watching }</td>
                                </tr>
                                <tr>
                                    <td>{ "Users bruteforcing" }</td>
                                    <td>{ self.users_bruteforcing }</td>
                                </tr>
                            </table>
                        </section>
                    </div>
                    <section id="images">
                        <h2 style="text-align: center;">{ "Images" }</h2>
                        <div id="gallery">
                            {
                                for self.images.iter().map(|image| html! {
                                    <a class="imgur-image-container" target="_blank" rel="noopener" referrerpolicy="no-referrer" href=format!("https://i.imgur.com/{}.png", image)>
                                        <img class="imgur-image" decoding="async" referrerpolicy="no-referrer" src=format!("https://i.imgur.com/{}.png", image) />
                                    </a>
                                })
                            }
                        </div>
                    </section>
                </main>
                <footer>
                </footer>
            </body>
        }
    }
}

fn main() {
    yew::start_app::<Model>();
}
