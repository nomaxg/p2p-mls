//! Much of the boilerplate copied from
//! https://github.com/libp2p/rust-libp2p/blob/master/examples/chat.rs

use async_std::{channel, io, prelude::*};
use colored::Colorize;
use futures::lock::Mutex;
use futures::StreamExt;
use libp2p::{
    floodsub::{self, Floodsub, FloodsubEvent},
    mdns::{Mdns, MdnsEvent},
    swarm::{SwarmBuilder, SwarmEvent},
    NetworkBehaviour, PeerId, Swarm,
};
use mls::cli::parse_stdin;
use mls::node::Node;
use openmls::prelude::{
    KeyPackage, MlsMessageOut, TlsDeserializeTrait, TlsSerializeTrait, Welcome,
};
use std::error::Error;
use std::sync::Arc;

#[async_std::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let node = Node::default();
    let id_keys = node.get_network_keypair();
    let peer_id = PeerId::from(id_keys.public());

    // Create a Swarm to manage peers and events.
    let mut swarm = SwarmBuilder::new(
        libp2p::development_transport(id_keys).await?,
        MyBehaviour {
            floodsub: Floodsub::new(peer_id),
            mdns: Mdns::new(Default::default()).await?,
        },
        peer_id,
    )
    .build();

    // Listen on all interfaces and whatever port the OS assigns
    swarm.listen_on("/ip4/0.0.0.0/tcp/0".parse()?)?;

    let (out_msg_sender, out_msg_receiver) = channel::unbounded();
    let (in_msg_sender, in_msg_receiver) = channel::unbounded();

    let cloned_out = out_msg_sender.clone();

    // Spawn away the event loop that will keep the swarm going.
    async_std::task::spawn(network_event_loop(swarm, out_msg_receiver, in_msg_sender));

    // For demonstration purposes, we create a dedicated task that handles incoming messages.
    let arc_node = Arc::new(Mutex::new(node));
    let cloned_arc_node = Arc::clone(&arc_node);
    async_std::task::spawn(async move {
        let mut in_msg_receiver = in_msg_receiver.fuse();

        loop {
            let (peer, message) = in_msg_receiver.select_next_some().await;
            let inner_node = &mut *cloned_arc_node.lock().await;
            let bytes_array: &[u8] = &message;

            if let Ok(key_package) = KeyPackage::try_from(bytes_array) {
                if inner_node.is_group_leader() {
                    let (msg_out, welcome) = inner_node.add_member_to_group(key_package);
                    let welcome_serialized = welcome.tls_serialize_detached().unwrap();
                    let msg_out_serialized = msg_out.tls_serialize_detached().unwrap();
                    cloned_out.send(welcome_serialized).await.unwrap();
                    cloned_out.send(msg_out_serialized).await.unwrap();
                    println!(
                    "Received key package from {:?}, added to group and sent back welcome message and join message for existing members",
                    peer
                );
                }
            } else if let Ok(msg_out) = MlsMessageOut::try_from_bytes(bytes_array) {
                match inner_node.parse_message(msg_out) {
                    Ok(msg) => {
                        if let Some(str_msg) = msg {
                            println!("{}:{}", peer.to_string().red(), str_msg.blue());
                        }
                    }
                    Err(_) => {
                        println!("Could not parse message");
                    }
                }
            } else if let Ok(welcome) = Welcome::tls_deserialize(&mut &*bytes_array) {
                if let Ok(()) = inner_node.join_existing_group(welcome) {
                    println!("Received welcome message from from {:?}", peer);
                } else {
                    println!("Could not join group");
                }
            } else {
                println!("Received: '{:?}' from {:?}", message, peer);
            }
        }
    });

    let mut stdin = io::BufReader::new(io::stdin()).lines();

    while let Some(Ok(line)) = stdin.next().await {
        let inner_node = &mut *arc_node.lock().await;
        match parse_stdin(inner_node, line) {
            Ok(msg) => {
                out_msg_sender.send(msg).await.unwrap();
            }
            Err(e) => {
                println!("{}", e);
            }
        }
    }

    Ok(())
}

/// Defines the event-loop of our application's network layer.
///
/// The event-loop handles some network events itself like mDNS and interacts with the rest
/// of the application via channels.
/// Conceptually, this is an actor-ish design.
async fn network_event_loop(
    mut swarm: Swarm<MyBehaviour>,
    receiver: channel::Receiver<Vec<u8>>,
    sender: channel::Sender<(PeerId, Vec<u8>)>,
) {
    // Create a Floodsub topic
    let chat = floodsub::Topic::new("chat");

    swarm.behaviour_mut().floodsub.subscribe(chat.clone());

    let mut receiver = receiver.fuse();

    loop {
        futures::select! {
            event = swarm.select_next_some() => {
                match event {
                    SwarmEvent::NewListenAddr { address, .. } => {
                        println!("Listening on {}", address);
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, endpoint,.. } => {
                        println!("Connected to {} on {}", peer_id, endpoint.get_remote_address());
                    }
                    SwarmEvent::ConnectionClosed { peer_id,.. } => {
                        println!("Disconnected from {}", peer_id);
                    }
                    SwarmEvent::Behaviour(MyOutEvent::Mdns(MdnsEvent::Discovered(list))) => {
                        for (peer, _) in list {
                            swarm.behaviour_mut().floodsub.add_node_to_partial_view(peer);
                        }
                    }
                    SwarmEvent::Behaviour(MyOutEvent::Mdns(MdnsEvent::Expired(list))) => {
                        for (peer, _) in list {
                            if !swarm.behaviour_mut().mdns.has_node(&peer) {
                                swarm.behaviour_mut().floodsub.remove_node_from_partial_view(&peer);
                            }
                        }
                    },
                    SwarmEvent::Behaviour(MyOutEvent::Floodsub(FloodsubEvent::Message(message))) if message.topics.contains(&chat) => {

                        sender.send((message.source, message.data)).await.unwrap();
                    },
                    _ => {} // ignore all other events
                }
            },
            message = receiver.select_next_some() => {
                swarm.behaviour_mut().floodsub.publish(chat.clone(), message);
            }
        }
    }
}

#[derive(NetworkBehaviour)]
#[behaviour(event_process = false, out_event = "MyOutEvent")]
struct MyBehaviour {
    floodsub: Floodsub,
    mdns: Mdns,
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
enum MyOutEvent {
    Floodsub(FloodsubEvent),
    Mdns(MdnsEvent),
}

impl From<FloodsubEvent> for MyOutEvent {
    fn from(event: FloodsubEvent) -> MyOutEvent {
        MyOutEvent::Floodsub(event)
    }
}

impl From<MdnsEvent> for MyOutEvent {
    fn from(event: MdnsEvent) -> MyOutEvent {
        MyOutEvent::Mdns(event)
    }
}
