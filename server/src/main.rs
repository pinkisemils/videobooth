use bytes::Buf;
use futures::{SinkExt, StreamExt};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tokio::{fs::File, io::BufReader};
// use tokio_stream::StreamExt;
use warp::{self, Filter, Rejection, Reply, Stream};

#[tokio::main]
async fn main() {
    let (button_tx, _button_rx) = tokio::sync::broadcast::channel(1);
    let event_tx = button_tx.clone();
    tokio::spawn(async move {
        // Get tokio's version of stdin, which implements AsyncRead
        let stdin = tokio::io::stdin();
        // Create a buffered wrapper, which implements BufRead
        let reader = BufReader::new(stdin);
        // Take a stream of lines from this
        let mut lines = reader.lines();
        while let Ok(Some(_next_line)) = lines.next_line().await {
            let _ = button_tx.send(true);
        }
    });

    let receiver = warp::path!("recording")
        .and(warp::filters::path::end())
        .and(warp::filters::method::post())
        .and(warp::filters::body::stream())
        .and_then(handle_recording);

    let index = warp::filters::path::end()
        .and(warp::filters::method::get())
        .and_then(index);

    let events = warp::path!("events")
        .and(warp::filters::ws::ws())
        .and(warp::any().map(move || event_tx.clone()))
        .and_then(
            |ws: warp::filters::ws::Ws, event_tx: tokio::sync::broadcast::Sender<bool>| async move {
                let event_tx = event_tx.clone();
                let mut socket_rx =
                    tokio_stream::wrappers::BroadcastStream::new(event_tx.subscribe()).fuse();
                Result::<_, Rejection>::Ok(ws.on_upgrade(move |ws| async move {
                    let mut ws = ws.fuse();
                    futures::select! {
                        btn = socket_rx.select_next_some() => {
                            if btn == Ok(true) {
                                if let Err(err) = ws.send(warp::ws::Message::text("p")).await {
                                    println!("websocket failed: {}", err);
                                    return;
                                }
                            }

                        },
                        rx_msg = ws.next() => {
                            if rx_msg.is_none() {
                                return;
                            }
                        },
                    };
                }))
            },
        );

    println!("Serving the video guest book booth");

    warp::serve(receiver.or(index).or(events))
        .run("127.0.0.1:8080".parse::<std::net::SocketAddr>().unwrap())
        .await;
}

async fn index() -> Result<impl Reply, Rejection> {
    Ok(warp::reply::html(
        tokio::fs::read("../index.html")
            .await
            .map_err(io_rejection)?,
    ))
}

fn io_rejection(err: std::io::Error) -> Rejection {
    println!("Failed to read from received body {}", err);
    warp::reject()
}

async fn handle_recording(
    mut bytes: impl Stream<Item = Result<impl Buf, warp::Error>> + Unpin,
) -> Result<impl Reply, Rejection> {
    let now = chrono::Utc::now().naive_utc().timestamp_millis();
    println!("Receiving a video file for {}", now);
    let rejection = |err: std::io::Error| {
        println!("failed to do I/O: {}", err);
        warp::reject()
    };
    let fname = format!("{}.webm", now);
    let mut file = File::create(&fname).await.map_err(rejection)?;
    let mut buf_writer = tokio::io::BufWriter::new(&mut file);
    while let Some(bytes) = bytes.next().await {
        buf_writer
            .write_all(
                &bytes
                    .map_err(|err| {
                        println!("Failed to read from received body {}", err);
                        warp::reject()
                    })?
                    .chunk(),
            )
            .await
            .map_err(rejection)?;
    }
    buf_writer.flush().await.map_err(io_rejection)?;
    file.sync_all().await.map_err(io_rejection)?;

    println!("Video '{}' saved", fname);

    Ok(warp::reply())
}
