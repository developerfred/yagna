use std::clone::Clone;
use std::collections::{hash_map::Entry, HashMap, HashSet};
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

use tokio::codec::{FramedRead, FramedWrite};
use tokio::net::TcpListener;
use tokio::prelude::*;
use tokio::sync::mpsc;

use failure::_core::fmt::Display;
use ya_sb_proto::{
    codec::{GsbMessage, GsbMessageDecoder, GsbMessageEncoder},
    *,
};

struct MessageDispatcher<A, B>
where
    A: Hash + Eq,
    B: Sink,
{
    senders: HashMap<A, mpsc::Sender<B::SinkItem>>,
}

impl<A, B> MessageDispatcher<A, B>
where
    A: Hash + Eq,
    B: Sink + Send + 'static,
    B::SinkItem: Send + 'static,
    B::SinkError: From<mpsc::error::RecvError> + Debug,
{
    fn new() -> Self {
        MessageDispatcher {
            senders: HashMap::new(),
        }
    }

    fn register(&mut self, addr: A, sink: B) -> failure::Fallible<()> {
        match self.senders.entry(addr) {
            Entry::Occupied(_) => Err(failure::err_msg("Sender already registered")),
            Entry::Vacant(entry) => {
                let (tx, rx) = mpsc::channel(1000);
                tokio::spawn(
                    sink.send_all(rx)
                        .map(|_| ())
                        .map_err(|e| eprintln!("Send failed: {:?}", e)),
                );
                entry.insert(tx);
                Ok(())
            }
        }
    }

    fn unregister(&mut self, addr: &A) -> failure::Fallible<()> {
        match self.senders.remove(addr) {
            None => Err(failure::err_msg("Sender not registered")),
            Some(_) => Ok(()),
        }
    }

    fn send_message<T>(&mut self, addr: &A, msg: T) -> failure::Fallible<()>
    where
        T: Into<B::SinkItem>,
    {
        match self.senders.get_mut(addr) {
            None => Err(failure::err_msg("Sender not registered")),
            Some(sender) => {
                let sender = sender.clone();
                tokio::spawn(
                    sender
                        .send(msg.into())
                        .map(|_| ())
                        .map_err(|e| eprintln!("Send failed: {:?}", e)),
                );
                Ok(())
            }
        }
    }
}

type ServiceId = String;
type RequestId = String;

fn is_valid_service_id(service_id: &ServiceId) -> bool {
    service_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '/' || c == '_' || c == '-')
}

struct PendingCall<A>
where
    A: Hash + Eq,
{
    caller_addr: A,
    service_id: ServiceId,
}

struct Router<A, B>
where
    A: Hash + Eq,
    B: Sink,
{
    dispatcher: MessageDispatcher<A, B>,
    registered_endpoints: HashMap<ServiceId, A>,
    reversed_endpoints: HashMap<A, HashSet<ServiceId>>,
    pending_calls: HashMap<RequestId, PendingCall<A>>,
    client_calls: HashMap<A, HashSet<RequestId>>,
    endpoint_calls: HashMap<ServiceId, HashSet<RequestId>>,
}

impl<A, B> Router<A, B>
where
    A: Hash + Eq + Display + Clone,
    B: Sink + Send + 'static,
    B::SinkItem: Send + 'static + From<GsbMessage>,
    B::SinkError: From<mpsc::error::RecvError> + Debug,
{
    fn new() -> Self {
        Router {
            dispatcher: MessageDispatcher::new(),
            registered_endpoints: HashMap::new(),
            reversed_endpoints: HashMap::new(),
            pending_calls: HashMap::new(),
            client_calls: HashMap::new(),
            endpoint_calls: HashMap::new(),
        }
    }

    fn connect(&mut self, addr: A, sink: B) -> failure::Fallible<()> {
        println!("Accepted connection from {}", addr);
        self.dispatcher.register(addr, sink)
    }

    fn disconnect(&mut self, addr: &A) -> failure::Fallible<()> {
        println!("Closed connection with {}", addr);
        self.dispatcher.unregister(addr)?;

        // IDs of all endpoints registered by this server
        let service_ids = match self.reversed_endpoints.entry(addr.clone()) {
            Entry::Occupied(entry) => entry.remove().into_iter().collect(),
            Entry::Vacant(_) => vec![],
        };

        service_ids.iter().for_each(|service_id| {
            self.registered_endpoints.remove(service_id);
        });

        // IDs of all pending call requests unanswered by this server
        let pending_call_ids: Vec<RequestId> = service_ids
            .into_iter()
            .filter_map(|service_id| match self.endpoint_calls.entry(service_id) {
                Entry::Occupied(entry) => Some(entry.remove().into_iter()),
                Entry::Vacant(_) => None,
            })
            .flatten()
            .collect();
        let pending_calls: Vec<(RequestId, PendingCall<A>)> = pending_call_ids
            .into_iter()
            .filter_map(|request_id| match self.pending_calls.entry(request_id) {
                Entry::Occupied(entry) => Some(entry.remove_entry()),
                Entry::Vacant(_) => None,
            })
            .collect();

        // Answer all pending calls with ServiceFailure reply
        pending_calls
            .into_iter()
            .for_each(|(request_id, pending_call)| {
                self.client_calls
                    .get_mut(&pending_call.caller_addr)
                    .unwrap()
                    .remove(&request_id);
                let msg = CallReply {
                    request_id,
                    code: CallReplyCode::ServiceFailure as i32,
                    reply_type: CallReplyType::Full as i32,
                    data: "Service disconnected".to_owned().into_bytes(),
                };
                match self.send_message(&pending_call.caller_addr, msg) {
                    Err(err) => eprintln!("Send message failed: {:?}", err),
                    _ => (),
                };
            });

        // Remove all pending calls coming from this client
        match self.client_calls.entry(addr.clone()) {
            Entry::Occupied(entry) => {
                entry.remove().drain().for_each(|request_id| {
                    let pending_call = self.pending_calls.remove(&request_id).unwrap();
                    self.endpoint_calls
                        .get_mut(&pending_call.service_id)
                        .unwrap()
                        .remove(&request_id);
                });
            }
            Entry::Vacant(_) => {}
        }

        Ok(())
    }

    fn send_message<T>(&mut self, addr: &A, msg: T) -> failure::Fallible<()>
    where
        T: Into<GsbMessage>,
    {
        self.dispatcher.send_message(addr, msg.into())
    }

    fn register_endpoint(&mut self, addr: &A, msg: RegisterRequest) -> failure::Fallible<()> {
        println!(
            "Received RegisterRequest from {}. service_id = {}",
            addr, &msg.service_id
        );
        let msg = if !is_valid_service_id(&msg.service_id) {
            RegisterReply {
                code: RegisterReplyCode::RegisterBadRequest as i32,
                message: "Illegal service ID".to_string(),
            }
        } else {
            match self.registered_endpoints.entry(msg.service_id.clone()) {
                Entry::Occupied(_) => RegisterReply {
                    code: RegisterReplyCode::RegisterConflict as i32,
                    message: "Service ID already registered".to_string(),
                },
                Entry::Vacant(entry) => {
                    entry.insert(addr.clone());
                    self.reversed_endpoints
                        .entry(addr.clone())
                        .or_insert_with(|| HashSet::new())
                        .insert(msg.service_id);
                    RegisterReply {
                        code: RegisterReplyCode::RegisteredOk as i32,
                        message: "Service successfully registered".to_string(),
                    }
                }
            }
        };
        println!("{}", msg.message);
        self.send_message(addr, msg)
    }

    fn unregister_endpoint(&mut self, addr: &A, msg: UnregisterRequest) -> failure::Fallible<()> {
        println!(
            "Received UnregisterRequest from {}. service_id = {}",
            addr, &msg.service_id
        );
        let msg = match self.registered_endpoints.entry(msg.service_id.clone()) {
            Entry::Occupied(entry) if entry.get() == addr => {
                entry.remove();
                self.reversed_endpoints
                    .get_mut(addr)
                    .ok_or(failure::err_msg("Address not found"))?
                    .remove(&msg.service_id);
                println!("Service successfully unregistered");
                UnregisterReply {
                    code: UnregisterReplyCode::UnregisteredOk as i32,
                }
            }
            _ => {
                println!("Service not registered or registered by another server");
                UnregisterReply {
                    code: UnregisterReplyCode::NotRegistered as i32,
                }
            }
        };
        self.send_message(addr, msg)
    }

    fn call(&mut self, caller_addr: &A, msg: CallRequest) -> failure::Fallible<()> {
        println!(
            "Received CallRequest from {}. caller = {}, address = {}, request_id = {}",
            caller_addr, &msg.caller, &msg.address, &msg.request_id
        );
        let server_addr = match self.pending_calls.entry(msg.request_id.clone()) {
            Entry::Occupied(_) => Err("CallRequest with this ID already exists".to_string()),
            Entry::Vacant(call_entry) => {
                // TODO: Prefix matching
                match self.registered_endpoints.get(&msg.address) {
                    None => Err("No service registered under given address".to_string()),
                    Some(addr) => {
                        call_entry.insert(PendingCall {
                            caller_addr: caller_addr.clone(),
                            service_id: msg.address.clone(),
                        });
                        self.endpoint_calls
                            .entry(msg.address.clone())
                            .or_insert_with(|| HashSet::new())
                            .insert(msg.request_id.clone());
                        self.client_calls
                            .entry(caller_addr.clone())
                            .or_insert_with(|| HashSet::new())
                            .insert(msg.request_id.clone());
                        Ok(addr.clone())
                    }
                }
            }
        };
        match server_addr {
            Ok(server_addr) => {
                println!("Forwarding CallRequest to {}", server_addr);
                self.send_message(&server_addr, msg)
            }
            Err(err) => {
                println!("{}", err);
                let msg = CallReply {
                    request_id: msg.request_id,
                    code: CallReplyCode::CallReplyBadRequest as i32,
                    reply_type: CallReplyType::Full as i32,
                    data: err.into_bytes(),
                };
                self.send_message(caller_addr, msg)
            }
        }
    }

    fn reply(&mut self, server_addr: &A, msg: CallReply) -> failure::Fallible<()> {
        println!(
            "Received CallReply from {} request_id = {}",
            server_addr, &msg.request_id
        );
        let caller_addr = match self.pending_calls.entry(msg.request_id.clone()) {
            Entry::Occupied(entry) => {
                let pending_call = entry.get();
                let caller_addr = pending_call.caller_addr.clone();
                if msg.reply_type == CallReplyType::Full as i32 {
                    self.endpoint_calls
                        .get_mut(&pending_call.service_id)
                        .ok_or(failure::err_msg("Service not found"))?
                        .remove(&msg.request_id);
                    self.client_calls
                        .get_mut(&pending_call.caller_addr)
                        .ok_or(failure::err_msg("Client not found"))?
                        .remove(&msg.request_id);
                    entry.remove_entry();
                }
                Ok(caller_addr)
            }
            Entry::Vacant(_) => Err("Unknown request ID"),
        };
        match caller_addr {
            Ok(addr) => self.send_message(&addr, msg),
            Err(err) => Ok(println!("{}", err)),
        }
    }

    fn handle_message(&mut self, addr: A, msg: GsbMessage) -> failure::Fallible<()> {
        match msg {
            GsbMessage::RegisterRequest(msg) => self.register_endpoint(&addr, msg),
            GsbMessage::UnregisterRequest(msg) => self.unregister_endpoint(&addr, msg),
            GsbMessage::CallRequest(msg) => self.call(&addr, msg),
            GsbMessage::CallReply(msg) => self.reply(&addr, msg),
            _ => Err(failure::err_msg(format!(
                "Unexpected message received: {:?}",
                msg
            ))),
        }
    }
}

fn main() {
    let listen_addr = "127.0.0.1:8245".parse().unwrap();
    let listener = TcpListener::bind(&listen_addr).expect("Unable to bind TCP listener");
    let router = Arc::new(Mutex::new(Router::new()));

    let server = listener
        .incoming()
        .map_err(|e| eprintln!("Accept failed: {:?}", e))
        .for_each(move |sock| {
            let addr = sock.peer_addr().unwrap();
            let (reader, writer) = sock.split();
            let writer = FramedWrite::new(writer, GsbMessageEncoder {});
            let reader = FramedRead::new(reader, GsbMessageDecoder::new());

            router
                .lock()
                .unwrap()
                .connect(addr.clone(), writer)
                .unwrap();
            let router1 = router.clone();
            let router2 = router.clone();

            tokio::spawn(
                reader
                    .from_err()
                    .for_each(move |msg| {
                        future::done(router1.lock().unwrap().handle_message(addr.clone(), msg))
                    })
                    .and_then(move |_| future::done(router2.lock().unwrap().disconnect(&addr)))
                    .map_err(|e| eprintln!("Error occurred handling message: {:?}", e)),
            )
        });

    tokio::run(server);
}
