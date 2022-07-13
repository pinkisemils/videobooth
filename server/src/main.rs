use bytes::Buf;
use tokio::fs::File;
use tokio::io::{AsyncWriteExt};
use tokio_stream::StreamExt;
use warp::{self, Filter, Rejection, Reply, Stream};

#[tokio::main]
async fn main() {
    println!("Hello, world!");

    let receiver = warp::path!("recording")
        .and(warp::filters::path::end())
        .and(warp::filters::method::post())
        .and(warp::filters::body::stream())
        .and_then(handle_recording);

    let index = warp::filters::path::end()
        .and(warp::filters::method::get())
        .and_then(index);

    warp::serve(receiver.or(index))
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
    println!("Failed to read from received body {err}");
    warp::reject()
}

async fn handle_recording(
    mut bytes: impl Stream<Item = Result<impl Buf, warp::Error>> + Unpin,
) -> Result<impl Reply, Rejection> {
    let now = chrono::Utc::now().naive_utc().timestamp_millis();
    let rejection = |err: std::io::Error| {
        println!("failed to do I/O: {err}");
        warp::reject()
    };
    let fname = format!("{now}.webm");
    let mut file = File::create(&fname).await.map_err(rejection)?;
    let mut buf_writer = tokio::io::BufWriter::new(&mut file);
    while let Some(bytes) = bytes.next().await {
        buf_writer
            .write_all(
                &bytes
                    .map_err(|err| {
                        println!("Failed to read from received body {err}");
                        warp::reject()
                    })?
                    .chunk(),
            )
            .await
            .map_err(rejection)?;
    }
    buf_writer.flush().await.map_err(io_rejection)?;
    file.sync_all().await.map_err(io_rejection)?;

    Ok(warp::reply())
}
