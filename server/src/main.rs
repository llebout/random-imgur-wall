use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use ws::{
    listen, CloseCode, Error as WSError, Handler, Handshake, Message, Request, Response, Result,
    Sender,
};

#[macro_use]
extern crate serde_derive;

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

struct User {
    is_bruteforcing: bool,
}

struct Server {
    users: Arc<Mutex<HashMap<u32, User>>>,
    out: Sender,
}

impl Handler for Server {
    fn on_request(&mut self, req: &Request) -> Result<(Response)> {
        match req.resource() {
            "/ws" => Response::from_request(req),
            _ => Ok(Response::new(404, "Not Found", b"404 - Not Found".to_vec())),
        }
    }

    fn on_open(&mut self, shake: Handshake) -> Result<()> {
        self.users.lock().unwrap().insert(
            self.out.connection_id(),
            User {
                is_bruteforcing: false,
            },
        );

        if let Ok(ws_message) = serde_json::to_string(&WsMessage {
            msg_type: WsMessageType::UsersWatching,
            text: None,
            number: Some(self.users.lock().unwrap().iter().count() as u64),
        }) {
            self.out.broadcast(Message::text(ws_message));
        }

        if let Ok(new_ws_message) = serde_json::to_string(&WsMessage {
            msg_type: WsMessageType::UsersBruteforcing,
            text: None,
            number: Some(
                self.users
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(id, user)| user.is_bruteforcing)
                    .count() as u64,
            ),
        }) {
            self.out.broadcast(Message::text(new_ws_message));
        }

        Ok(())
    }

    fn on_error(&mut self, err: WSError) {
        self.users.lock().unwrap().remove(&self.out.connection_id());

        if let Ok(ws_message) = serde_json::to_string(&WsMessage {
            msg_type: WsMessageType::UsersWatching,
            text: None,
            number: Some(self.users.lock().unwrap().iter().count() as u64),
        }) {
            self.out.broadcast(Message::text(ws_message));
        }

        if let Ok(new_ws_message) = serde_json::to_string(&WsMessage {
            msg_type: WsMessageType::UsersBruteforcing,
            text: None,
            number: Some(
                self.users
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(id, user)| user.is_bruteforcing)
                    .count() as u64,
            ),
        }) {
            self.out.broadcast(Message::text(new_ws_message));
        }
    }

    fn on_close(&mut self, code: CloseCode, reason: &str) {
        self.users.lock().unwrap().remove(&self.out.connection_id());

        if let Ok(ws_message) = serde_json::to_string(&WsMessage {
            msg_type: WsMessageType::UsersWatching,
            text: None,
            number: Some(self.users.lock().unwrap().iter().count() as u64),
        }) {
            self.out.broadcast(Message::text(ws_message));
        }

        if let Ok(new_ws_message) = serde_json::to_string(&WsMessage {
            msg_type: WsMessageType::UsersBruteforcing,
            text: None,
            number: Some(
                self.users
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|(id, user)| user.is_bruteforcing)
                    .count() as u64,
            ),
        }) {
            self.out.broadcast(Message::text(new_ws_message));
        }
    }

    fn on_message(&mut self, msg: Message) -> Result<()> {
        if let Ok(text) = msg.as_text() {
            if let Ok(ws_message) = serde_json::from_str::<WsMessage>(&text) {
                match ws_message.msg_type {
                    WsMessageType::New => {
                        if let Some(text) = ws_message.text {
                            if let Ok(new_ws_message) = serde_json::to_string(&WsMessage {
                                msg_type: WsMessageType::New,
                                text: Some(text),
                                number: None,
                            }) {
                                self.out.broadcast(Message::text(new_ws_message));
                            }
                        }
                    }
                    WsMessageType::Start => {
                        let mut users = self.users.lock().unwrap();

                        if let Some(user) = users.get_mut(&self.out.connection_id()) {
                            user.is_bruteforcing = true;

                            if let Ok(new_ws_message) = serde_json::to_string(&WsMessage {
                                msg_type: WsMessageType::UsersBruteforcing,
                                text: None,
                                number: Some(
                                    users
                                        .iter()
                                        .filter(|(id, user)| user.is_bruteforcing)
                                        .count() as u64,
                                ),
                            }) {
                                self.out.broadcast(Message::text(new_ws_message));
                            }
                        }
                    }
                    WsMessageType::Stop => {
                        let mut users = self.users.lock().unwrap();

                        if let Some(user) = users.get_mut(&self.out.connection_id()) {
                            user.is_bruteforcing = false;

                            if let Ok(new_ws_message) = serde_json::to_string(&WsMessage {
                                msg_type: WsMessageType::UsersBruteforcing,
                                text: None,
                                number: Some(
                                    users
                                        .iter()
                                        .filter(|(id, user)| user.is_bruteforcing)
                                        .count() as u64,
                                ),
                            }) {
                                self.out.broadcast(Message::text(new_ws_message));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }
}

fn main() {
    env_logger::init();

    let listen_addr = env::var("WS_LISTEN_ADDR").expect("WS_LISTEN_ADDR must be defined.");

    let users = Arc::new(Mutex::new(HashMap::new()));

    listen(listen_addr, |out| Server {
        out,
        users: users.clone(),
    })
    .unwrap();
}
