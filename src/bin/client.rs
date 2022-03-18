extern crate tun;
use anyhow::Result;
use bytes::Bytes;
use clap::{App, AppSettings, Arg};
use std::io::Read;
use std::io::Write;
use std::sync::Arc;
use std::sync::Mutex;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

#[tokio::main]
async fn main() -> Result<()> {
    let mut app = App::new("data-channels")
        .version("0.1.0")
        .author("Rain Liu <yliu@webrtc.rs>")
        .about("An example of Data-Channels.")
        .setting(AppSettings::DeriveDisplayOrder)
        .setting(AppSettings::SubcommandsNegateReqs)
        .arg(
            Arg::with_name("FULLHELP")
                .help("Prints more detailed help information")
                .long("fullhelp"),
        )
        .arg(
            Arg::with_name("debug")
                .long("debug")
                .short("d")
                .help("Prints debug log information"),
        );

    let matches = app.clone().get_matches();

    if matches.is_present("FULLHELP") {
        app.print_long_help().unwrap();
        std::process::exit(0);
    }

    let debug = matches.is_present("debug");
    if debug {
        env_logger::Builder::new()
            .format(|buf, record| {
                writeln!(
                    buf,
                    "{}:{} [{}] {} - {}",
                    record.file().unwrap_or("unknown"),
                    record.line().unwrap_or(0),
                    record.level(),
                    chrono::Local::now().format("%H:%M:%S.%6f"),
                    record.args()
                )
            })
            .filter(None, log::LevelFilter::Trace)
            .init();
    }

    let mut m = MediaEngine::default();

    m.register_default_codecs()?;
    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut m).await?;

    // Create the API object with the MediaEngine
    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();

    // Prepare the configuration
    let config = RTCConfiguration {
        ice_servers: vec![
            RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            },
            RTCIceServer {
                urls: vec!["turn:numb.viagenie.ca".to_owned()],
                username: String::from("s@sy.sa"),
                credential: String::from("Aa@123456"),
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    // Create a new RTCPeerConnection
    let peer_connection = Arc::new(api.new_peer_connection(config).await?);
    let (done_tx, mut done_rx) = tokio::sync::mpsc::channel::<()>(1);

    // Set the handler for Peer connection state
    // This will notify you when the peer has connected/disconnected
    peer_connection
        .on_peer_connection_state_change(Box::new(move |status: RTCPeerConnectionState| {
            println!("Peer Connection State has changed: {}", status);
            match status {
                RTCPeerConnectionState::Failed => {
                    println!("Peer Connection has gone to failed exiting");
                    let _ = done_tx.try_send(());
                }
                RTCPeerConnectionState::Closed => {
                    std::process::exit(0);
                }
                _ => {}
            }
            Box::pin(async {})
        }))
        .await;
    let mut config = tun::Configuration::default();
    config
        .address((10, 25, 0, 1))
        .netmask((255, 255, 255, 0))
        .mtu(1200)
        .up();

    #[cfg(target_os = "linux")]
    config.platform(|config| {
        config.packet_information(true);
    });

    // Register data channel creation handling
    peer_connection
        .on_data_channel(Box::new(move |data_channel: Arc<RTCDataChannel>| {
            let device: tun::platform::Device = tun::create(&config).unwrap();
            let device_clone = Arc::new(Mutex::new(device));
            Box::pin(async move {
                // data_channel
                //     .on_message(Box::new(move |msg: DataChannelMessage| {
                //         Box::pin(async move {
                //             println!(
                //                 "receive message: {}",
                //                 String::from_utf8(msg.data.to_vec()).unwrap()
                //             );
                //         })
                //     }))
                //     .await;

                let device_clone = device_clone.clone();
                let dc = data_channel.clone();
                data_channel
                    .on_open(Box::new(move || {
                        Box::pin(async move {
                            println!(
                                "datachannel label: {}, id: {} is open",
                                dc.label(),
                                dc.id()
                            );

                            loop {
                                let mut buf: [u8; 500] = [0u8; 500];
                                let amount = device_clone.lock().unwrap().read(&mut buf).unwrap();
                                let dc2 = dc.clone();
                                tokio::task::spawn(async move {
                                    let result: Result<usize> = dc2
                                    .send(&Bytes::from(buf.to_vec()).slice(0..amount))
                                    .await
                                    .map_err(Into::into);
                                    println!("send\t\t: {:?}\nresult\t\t: Len({:?})", &buf[0..amount], result.unwrap())                                    
                                });
                            }
                        })
                    }))
                    .await;
            })
        }))
        .await;

    // Wait for the offer to be pasted
    let line = signal::must_read_stdin()?;
    let desc_data = signal::decode(line.as_str())?;
    let offer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;

    // Set the remote SessionDescription
    peer_connection.set_remote_description(offer).await?;

    // Create an answer
    let answer = peer_connection.create_answer(None).await?;

    // Create channel that is blocked until ICE Gathering is complete
    let mut gather_complete = peer_connection.gathering_complete_promise().await;

    // Sets the LocalDescription, and starts our UDP listeners
    peer_connection.set_local_description(answer).await?;

    // Block until ICE Gathering is complete, disabling trickle ICE
    // we do this because we only can exchange one signaling message
    // in a production application you should exchange ICE Candidates via OnICECandidate
    let _ = gather_complete.recv().await;

    // Output the answer in base64 so we can paste it in browser
    if let Some(local_desc) = peer_connection.local_description().await {
        let json_str = serde_json::to_string(&local_desc)?;
        let b64 = signal::encode(&json_str);
        println!("{}", b64);
    } else {
        println!("generate local_description failed!");
    }

    println!("Press ctrl-c to stop");
    tokio::select! {
        _ = done_rx.recv() => {
            println!("received done signal!");
        }
        _ = tokio::signal::ctrl_c() => {
            println!("");
        }
    };

    peer_connection.close().await?;

    Ok(())
}
